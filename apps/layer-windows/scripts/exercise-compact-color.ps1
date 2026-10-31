param([Parameter(Mandatory)][string]$Executable,[ValidateSet('full','dark','light','input','pair-dark','pair-light','slot-menu-dark','slot-menu-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
$CapyFind='visible'
$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
$CapyCaptureDelay=250
$slotMenuOnly=$Journey.StartsWith('slot-menu-')
$theme=if($Journey -in @('light','pair-light','slot-menu-light')){'light'}else{'dark'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/compact-color/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
function Paint { (Model).state.colors.foreground|ConvertTo-Json -Compress }
function Shape([string]$Shape){
    if((Model).color_panel.shape -eq $Shape){return}
    $space=switch($Shape){circle{'Okhsv'} square{'HSV'} triangle{'HLS'}}
    $paint=Paint;Invoke "Use $space $Shape" -Name
    Wait-Until {(Model).color_panel.shape -eq $Shape} "Shape did not change to $Shape"
    if((Paint) -ne $paint){throw 'Changing picker projection changed the paint'}
}
function Point([double]$X,[double]$Y){
    $bounds=(Control 'color-wheel' -Arranged).Current.BoundingRectangle
    @([int][Math]::Round($bounds.X+$X*$bounds.Width),[int][Math]::Round($bounds.Y+$Y*$bounds.Height))
}
function Pick-Field([string]$Device,[double]$X,[double]$Y){
    $before=Paint;$start=Point .5 .5;$end=Point $X $Y
    $bounds=(Control 'color-wheel' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Down($Device,$start[0],$start[1]);[CapyRowPointer]::Move($end[0],$end[1]);[CapyRowPointer]::Up()
    Wait-Until {
        $marker=(Model).color_panel.wheel_marker
        [Math]::Abs($bounds.X+$marker[0]*$bounds.Width-$end[0]) -le 1.5 -and
        [Math]::Abs($bounds.Y+$marker[1]*$bounds.Height-$end[1]) -le 1.5
    } "$Device did not pick the final field position"
    Wait-Until {(Control 'color-controls').Current.ItemStatus -eq 'Ready'} "$Device did not release the wheel"
    if((Paint) -eq $before){throw "$Device did not change the field paint"}
}
function Swatch-Point([string]$Id){
    $b=(Control $Id -Arranged).Current.BoundingRectangle
    @([int][Math]::Round($b.X+$b.Width/2),[int][Math]::Round($b.Y+$b.Height/2))
}
function Swatch-Tap([string]$Device,$At){
    Start-Sleep -Milliseconds ([Windows.Forms.SystemInformation]::DoubleClickTime+100)
    [CapyRowPointer]::Down($Device,$At[0],$At[1]);[CapyRowPointer]::Up()
}
function Park-Pointer{
    $at=Point .5 .5;[CapyRowPointer]::Hover($at[0],$at[1])
    Start-Sleep -Milliseconds 250
}
function Hit-Path($At){
    $hit=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($At[0],$At[1]))
    if($hit.Current.ProcessId -ne $review.Id){throw 'Swatch hit is outside the owned review'}
    $path=[Collections.Generic.List[object]]::new();$seen=[Collections.Generic.HashSet[string]]::new()
    while($hit){
        $current=$hit.Current;$runtime=$hit.GetRuntimeId() -join ':';$bounds=$current.BoundingRectangle
        if(!$seen.Add($runtime)){throw 'The native hit ancestor path contains a cycle'}
        $path.Add([pscustomobject]@{id=$current.AutomationId;name=$current.Name;process=$current.ProcessId;runtime_id=$runtime;type=$current.ControlType.ProgrammaticName;class=$current.ClassName;enabled=$current.IsEnabled;offscreen=$current.IsOffscreen;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)})
        $hit=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($hit)
    }
    $path.ToArray()
}
function Hit-Id($At){
    @((Hit-Path $At)|Where-Object {$_.id -like 'color-*'})[0].id
}
function Circle-Contains($Bounds,$At,[double]$Inset=-1){
    $dx=$At[0]-$Bounds.X-$Bounds.Width/2;$dy=$At[1]-$Bounds.Y-$Bounds.Height/2
    $dx*$dx+$dy*$dy -le [Math]::Pow($Bounds.Width/2-$Inset,2)
}
function Rim-Points($Bounds,[double]$Scale,[double]$Angle){
    $cx=$Bounds.X+$Bounds.Width/2;$cy=$Bounds.Y+$Bounds.Height/2;$outer=$Bounds.Width/2;$inner=$outer-2*$Scale
    $middle=($outer+$inner)/2
    $x=$cx+$middle*[Math]::Cos($Angle);$y=$cy+$middle*[Math]::Sin($Angle)
    $points=@()
    for($py=[int][Math]::Floor($y)-2;$py -le [Math]::Floor($y)+2;$py++){
        for($px=[int][Math]::Floor($x)-2;$px -le [Math]::Floor($x)+2;$px++){
            $dx=$px+.5-$cx;$dy=$py+.5-$cy;$radius=[Math]::Sqrt($dx*$dx+$dy*$dy)
            $across=[Math]::Abs($dx*[Math]::Sin($Angle)-$dy*[Math]::Cos($Angle))
            if($radius -gt $inner -and $radius -lt $outer -and $across -le 2){$points+=,@($px,$py)}
        }
    }
    $points
}
function Overlap-Point{
    $a=(Control 'color-foreground' -Arranged).Current.BoundingRectangle
    $b=(Control 'color-background' -Arranged).Current.BoundingRectangle
    $x=$a.X+$a.Width/2;$y=$a.Y+$a.Height/2;$dx=$b.X+$b.Width/2-$x;$dy=$b.Y+$b.Height/2-$y
    $distance=[Math]::Sqrt($dx*$dx+$dy*$dy);$along=($distance-$b.Width/2+$a.Width/2)/2
    @([int][Math]::Round($x+$dx*$along/$distance),[int][Math]::Round($y+$dy*$along/$distance))
}
function Swatch-Pixel($Bitmap,$At){
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen([IntPtr]$root.Current.NativeWindowHandle,[ref]$origin)){throw 'Swatch capture origin is unavailable'}
    $pixel=$Bitmap.GetPixel($At[0]-$origin.x,$At[1]-$origin.y)
    @([int]$pixel.R,[int]$pixel.G,[int]$pixel.B)
}
function Assert-Front([string]$Slot,[string]$Name){
    $snapshot=Model
    if($snapshot.color_panel.front_swatch -ne $Slot){throw "$Name lost shared front swatch $Slot"}
    $at=Overlap-Point;Capture $Name -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{
        $actual=Swatch-Pixel $bitmap $at
        $origin=[CapyRowPointer+Point]::new()
        $handle=[IntPtr]$root.Current.NativeWindowHandle
        [CapyRowPointer]::ClientToScreen($handle,[ref]$origin)|Out-Null
        $geometry=[ordered]@{point=$at;rgb=$actual;drawing_hwnd=$handle.ToInt64();drawing_origin=@($origin.x,$origin.y);swatches=@(foreach($id in @('color-foreground','color-background')){$b=(Control $id -Arranged).Current.BoundingRectangle;@{id=$id;bounds=@($b.X,$b.Y,$b.Width,$b.Height)}})}
        [IO.File]::WriteAllText((Join-Path $run ($Name+'-sample.json')),($geometry|ConvertTo-Json -Depth 5))
        $rgba=($snapshot.color_panel.swatches|Where-Object slot -eq $Slot).rgba
        for($i=0;$i -lt 3;$i++){if([Math]::Abs($actual[$i]-$rgba[$i]*255) -gt 8){throw "$Name overlap shows $($actual -join ',') instead of $Slot paint"}}
    }finally{$bitmap.Dispose()}
    @{point=$at;ui_automation=(Hit-Id $at);front=$Slot}|ConvertTo-Json|Set-Content (Join-Path $run ($Name+'-hit.json'))
}
function Assert-Rim([string]$Id,[string]$Name){
    if($Journey -eq 'input'){return}
    $bounds=(Control $Id -Arranged).Current.BoundingRectangle
    $scale=[CapyRowPointer]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)/96.
    $snapshot=Model
    $front='color-'+$snapshot.color_panel.front_swatch
    $order=@('color-background','color-foreground','color-transparent')|Where-Object {$_ -ne $front}
    $order=@($order)+@($front);$index=[Array]::IndexOf($order,$Id)
    $covers=@($order|Select-Object -Skip ($index+1)|ForEach-Object {(Control $_ -Arranged).Current.BoundingRectangle})
    $ink=[Drawing.ColorTranslator]::FromHtml($snapshot.state.palette.text)
    Capture $Name -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')));$samples=@()
    try{
        for($i=0;$i -lt 16;$i++){
            $angle=$i*[Math]::PI/8
            $sector=@()
            foreach($at in (Rim-Points $bounds $scale $angle)){
                if(@($covers|Where-Object {Circle-Contains $_ $at}).Count){continue}
                $pixel=Swatch-Pixel $bitmap $at
                $difference=[Math]::Max([Math]::Abs($pixel[0]-$ink.R),[Math]::Max([Math]::Abs($pixel[1]-$ink.G),[Math]::Abs($pixel[2]-$ink.B)))
                $sector+=@{point=$at;rgb=$pixel;difference=$difference}
            }
            if(!$sector.Count){continue}
            $best=$sector[0]
            foreach($candidate in $sector){if([double]$candidate.difference -lt [double]$best.difference){$best=$candidate}}
            $samples+=,@{angle=$angle;pixels=$sector}
            if($best.difference -gt 40){throw "$Name has no visible outline in rim sector ${i}: $($best.rgb -join ',')"}
        }
        if($samples.Count -lt 6){throw "$Name has too few visible rim samples"}
    }finally{
        $bitmap.Dispose()
        @{bounds=@{x=$bounds.X;y=$bounds.Y;width=$bounds.Width;height=$bounds.Height};scale=$scale;samples=$samples}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ($Name+'-pixels.json'))
    }
}
function Pair-Command([string]$Id,[string]$Menu='Window'){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
    Invoke $Id
}
function Pair-Choice([string]$Id,[string]$Name){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Pair-Text([string]$Id,[string]$Value){
    $control=Control $Id;$pattern=$null
    if(!$control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){
        $control=$control.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))
        if(!$control -or $control.Current.ProcessId -ne $review.Id){throw "No owned editor for $Id"}
        $pattern=$control.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    }
    $pattern.SetValue($Value)
}
function Quick-Color([string]$Label){
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x4B)
    Wait-Until {$search=Find 'command-search';$search -and !$search.Current.IsOffscreen} 'Command search did not open'
    (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Label)
    Wait-Until {$row=Find 'command-result-0';$row -and $row.Current.Name -eq $Label} "Command search did not rank $Label first"
    [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {$search=Find 'command-search';!$search -or $search.Current.IsOffscreen} 'Command search did not close'
}
function Pair-Park{
    $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
}
function Pair-Controls{
    $snapshot=Model
    $panel=@($snapshot.panels|Where-Object id -eq 'toolbar')[0]
    $tile=@($panel.tiles|Where-Object {$_.control.kind -eq 'color'})[0]
    $geometry=@($snapshot.layout.groups|Where-Object active -eq 'toolbar')[0].tiles
    $entry=@($snapshot.header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind -eq 'color'})[0]
    if(!$tile -or !$entry -or !$geometry){throw 'Paint pair fixture requires actual toolbar and header color controls'}
    $frame=Control ('header-item-'+$entry.id) -Arranged
    $button=$frame.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
    $size=@($snapshot.header.sizes|Where-Object id -eq $snapshot.header.model.size)[0].icon
    $tab=Control 'panel-tab-color' -Arranged
    $named=$tab.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
    @(@{kind='header';button=$button;size=$size;center=$null},
        @{kind='toolbar';button=(Control ('tile-toolbar-'+$tile.id) -Arranged);size=$geometry.tile_icon_size;center=$(if($geometry.tile_label_lines -gt 0){18}else{$null})},
        @{kind='tab';button=$tab;size=16;center=$null;title=$(if($named -and !$named.Current.IsOffscreen){$named}else{$null})})
}
function Pair-Setup{
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $root.Current.NativeWindowHandle -StateDirectory $run -Action Resize -Width 1800 -Height 1300
    Pair-Command 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Header editor did not open for color icon fixture'
    $from=(Control 'header-component-tools' -Arranged).Current.BoundingRectangle
    $presentation=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json;$zone=$presentation.geometry.zones[1]
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen([IntPtr]$root.Current.NativeWindowHandle,[ref]$origin)){throw 'Header fixture origin is unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)/96.
    $labels=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'menu_labels'})[0]
    [CapyRowPointer]::Hover([int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
    Focus-Keyboard (Control ('header-select-'+$labels.id) -Arranged)
    $tip=@{element=$null}
    Wait-Until {$tip.element=@(Capy-Tooltips $review.Id|Where-Object {$_.Current.Name -eq (Control ('header-item-'+$labels.id)).Current.Name})[0];$tip.element -and $tip.element.Current.BoundingRectangle.Contains([double]($from.X+$from.Width/2),[double]($from.Y+$from.Height/2))} 'Menu Labels tooltip did not cover the Add Tools press'
    [CapyRowPointer]::Down('mouse',[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
    [CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width/2)*$scale),[int]($origin.y+($zone.y+$zone.height/2)*$scale))
    Wait-Until {try{$gesture=(Control 'title-bar').Current.HelpText|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $gesture.preview.target}catch{$false}} 'Header color insertion has no target'
    [CapyRowPointer]::Up();Wait-Until {(Model).picker} 'Header color picker did not open'
    Pair-Text 'tool-picker-search' 'choose current paint color'
    Wait-Until {Find 'picker-choice-color-0'} 'Header picker omitted Color'
    (Control 'picker-choice-color-0').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    $confirm=@{button=$null}
    Wait-Until {$picker=(Model).picker;$confirm.button=Find 'Add Tools' -Name -Type ([System.Windows.Automation.ControlType]::Button) -Visible;$picker.selected_count -eq 1 -and $picker.can_confirm -and $confirm.button -and $confirm.button.Current.IsEnabled} 'Header picker did not enable Add Tools for its selected Color'
    $confirm.button=Control 'Add Tools' -Name -Type ([System.Windows.Automation.ControlType]::Button) -Arranged
    if(!$confirm.button.Current.IsEnabled -or $confirm.button.Current.ProcessId -ne $review.Id){throw 'Add Tools is not enabled in the owned picker'}
    $confirm.button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Model).picker} 'Header color picker did not close'
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Header color editor did not finish'
    $script:pairIdentities=@{}
    foreach($control in (Pair-Controls)|Where-Object {$_.kind -ne 'tab'}){$script:pairIdentities[$control.kind]=$control.button.GetRuntimeId() -join ':'}
}
function Pair-Glyph($Bitmap,$Control,$Pair,[double]$Left,[double]$Top,[double]$Unit){
    $samples=@()
    foreach($slot in @('foreground','background')){
        $swatch=@($Pair.swatches|Where-Object slot -eq $slot)[0]
        $center=if($slot -eq 'foreground'){6.75}else{11.};$radius=if($slot -eq 'foreground'){6.}else{4.25}
        $otherCenter=if($slot -eq 'foreground'){11.}else{6.75};$otherRadius=if($slot -eq 'foreground'){4.25}else{6.}
        $seen=@{};$count=0
        for($y=0;$y -lt 16;$y++){
            for($x=0;$x -lt 16;$x++){
                $at=@([int][Math]::Floor($Left+($x+.5)*$Unit),[int][Math]::Floor($Top+($y+.5)*$Unit))
                $cx=($at[0]+.5-$Left)/$Unit;$cy=($at[1]+.5-$Top)/$Unit
                if([Math]::Sqrt([Math]::Pow($cx-$center,2)+[Math]::Pow($cy-$center,2)) -ge $radius-1.5){continue}
                if($slot -ne $Pair.front_swatch -and [Math]::Sqrt([Math]::Pow($cx-$otherCenter,2)+[Math]::Pow($cy-$otherCenter,2)) -le $otherRadius+1){continue}
                if($swatch.rgba[3] -lt 1){
                    $cell=$Pair.checker_cell;$dx=($cx-$center+$radius)%$cell;$dy=($cy-$center+$radius)%$cell
                    if([Math]::Min($dx,$cell-$dx)*$Unit -lt 1.5 -or [Math]::Min($dy,$cell-$dy)*$Unit -lt 1.5){continue}
                }
                $pixel=Swatch-Pixel $Bitmap $at;$best=255;$which=-1
                for($i=0;$i -lt 2;$i++){
                    $difference=0;for($channel=0;$channel -lt 3;$channel++){$difference=[Math]::Max($difference,[Math]::Abs($pixel[$channel]-255*$swatch.checker[$i][$channel]))}
                    if($difference -lt $best){$best=$difference;$which=$i}
                }
                $samples+=@{control=$Control.kind;slot=$slot;point=$at;canonical=@($cx,$cy);origin=@($Left,$Top);unit=$Unit;rgb=$pixel;difference=$best}
                if($best -gt 8){return @{samples=$samples;failure="$($Control.kind) $slot glyph shows $($pixel -join ',') outside its shared opaque checker colors"}}
                $seen[$which]=$true;$count++
            }
        }
        if($count -lt 6){return @{samples=$samples;failure="has too few visible $slot icon samples"}}
        if([Math]::Abs($swatch.checker[0][0]-$swatch.checker[1][0])*255 -gt 16 -and $seen.Count -ne 2){return @{samples=$samples;failure="$slot glyph omitted a shared checker color"}}
    }
    @{samples=$samples;failure=$null}
}
function Assert-Pair([string]$Name){
    Pair-Park;$snapshot=Model;$pair=$snapshot.paint_pair
    if(!$pair -or $pair.swatches.Count -ne 2){throw 'Shared paint pair view is missing'}
    $controls=Pair-Controls;Capture $Name -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')));$samples=@()
    try{
        foreach($control in $controls){
            if($script:pairIdentities.ContainsKey($control.kind) -and ($control.button.GetRuntimeId() -join ':') -ne $script:pairIdentities[$control.kind]){throw "$Name replaced retained $($control.kind) color button"}
            $bounds=$control.button.Current.BoundingRectangle
            $scale=[CapyRowPointer]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)/96.;$unit=$control.size*$scale/16.
            if($unit -le 0){throw "Missing canonical icon size for $($control.kind)"}
            $left=if($null -ne $control.center){$bounds.X+$control.center*$scale-8*$unit}else{$bounds.X+$bounds.Width/2-8*$unit}
            $top=$bounds.Y+$bounds.Height/2-8*$unit;$slack=0
            if($control.title){$title=$control.title.Current.BoundingRectangle;$left=$title.X-6*$scale-16*$unit;$slack=2}
            $offsets=@(foreach($dy in -$slack..$slack){foreach($dx in -$slack..$slack){@{x=$dx;y=$dy}}})|Sort-Object {[Math]::Abs($_.x)+[Math]::Abs($_.y)}
            $glyph=$null
            foreach($offset in $offsets){
                $attempt=Pair-Glyph $bitmap $control $pair ($left+$offset.x) ($top+$offset.y) $unit
                if(!$glyph){$glyph=$attempt}
                if(!$attempt.failure){$glyph=$attempt;break}
            }
            $samples+=$glyph.samples
            if($glyph.failure){throw "$Name $($glyph.failure)"}
        }
    }finally{$bitmap.Dispose();$samples|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ($Name+'-pixels.json'))}
}
function Pair-Value([string]$Id,[string]$Value){
    Invoke $Id;$entry=Control ($Id+'-entry');$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    [CapyRowPointer]::Key([uint32]$review.Id,0x0D);Wait-Until {!(Find ($Id+'-entry'))} "Edit Color did not commit $Id"
}
function Pair-SetPaint([string[]]$Channels,[string]$Intensity=''){
    $before=(Model).paint_pair.definition|ConvertTo-Json -Compress
    Invoke 'color-edit';Wait-Until {Find 'edit-color-apply'} 'Edit Color did not open'
    Invoke 'edit-color-form-0';Invoke 'edit-color-form-0-linear_rgb'
    Wait-Until {(Control 'edit-color-form-0').Current.Name -eq 'Linear RGB'} 'Edit Color did not switch to Linear RGB'
    for($i=0;$i -lt 3;$i++){Pair-Value "edit-color-0-$i" $Channels[$i]}
    if($Intensity){
        Pair-Value 'edit-color-intensity' $Intensity
        $range=Control 'edit-color-intensity-range'
        $rangePattern=$range.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
        $rangePattern.SetValue(1)
        Wait-Until {$rangePattern.Current.Value -eq 1} 'Edit Color intensity did not accept its accessible range action'
        $range.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x24)
        Wait-Until {$rangePattern.Current.Value -eq 0} 'Edit Color intensity Home did not restore zero EV'
        Capture ('pair-intensity-keyboard-'+$theme)
        $rangePattern.SetValue([double]$Intensity)
        Wait-Until {$rangePattern.Current.Value -eq [double]$Intensity} 'Edit Color intensity did not restore the requested HDR value'
    }
    Invoke 'edit-color-apply'
    Wait-Until {$pair=(Model).paint_pair;$pair -and $pair.definition.rgba -and (($pair.definition|ConvertTo-Json -Compress) -ne $before)} 'Paint definition did not publish'
    Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'
}
function Slot-Colors{
    $colors=(Model).state.colors
    [ordered]@{foreground=$colors.foreground;background=$colors.background;temporary=$colors.temporary;slot=$colors.slot;paint_slot=$colors.paint_slot}|ConvertTo-Json -Depth 8 -Compress
}
function Slot-Checkpoint{
    $state=(Model).state
    [ordered]@{document=($state.document_file|Select-Object epoch,revision,modified);layers=@($state.layers|Select-Object id,paint_revision,mask_revision,visible,locked,opacity,blend);history=@($state.commands|Where-Object {$_.id -in @('undo','redo')}|Select-Object id,label,enabled)}|ConvertTo-Json -Depth 12 -Compress
}
function Slot-Park{
    $box=(Control 'color-readout' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2))
}
function Slot-Pixels([string]$Name=''){
    $path=if($Name){Join-Path $run ($Name+'.png')}else{''}
    Screen-Pixels $script:slotPixelPoint[0] $script:slotPixelPoint[1] $path
}
function Slot-Receipt([string]$Name,[string]$Expected,[string]$Observed){
    $captured=Slot-Pixels $Name
    $model=Model
    [ordered]@{expected_pixels=$Expected;observed_pixels=$Observed;captured_pixels=$captured;point=$script:slotPixelPoint;checkpoint=(Slot-Checkpoint);colors=(Slot-Colors);camera=$model.state.camera;readiness=($model|Select-Object gpu_ready,canvas_ready,brush_ready,shaders_ready)}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($Name+'.json'))
}
function Slot-ColorPanel{
    if(!(Find 'color-foreground' -Visible)){
        Invoke $(if(Find 'column-icon-color' -Visible){'column-icon-color'}elseif(Find 'drawer-tab-color' -Visible){'drawer-tab-color'}else{'panel-tab-color'})
    }
    $null=Control 'color-foreground' -Arranged
    $null=Control 'color-background' -Arranged
}
function Slot-Hit([string]$Device,[string]$Slot,[string]$Receipt=''){
    $button=Control ('color-'+$Slot) -Arranged;$bounds=$button.Current.BoundingRectangle
    $expected=$button.GetRuntimeId() -join ':'
    $front=(Model).color_panel.front_swatch
    $cover=if($front -ne $Slot){(Control ('color-'+$front) -Arranged).Current.BoundingRectangle}else{$null}
    $trace=[ordered]@{device=$Device;slot=$Slot;expected_id=('color-'+$Slot);expected_runtime_id=$expected;expected_bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);front_swatch=$front;covering_circle=$(if($cover){@($cover.X,$cover.Y,$cover.Width,$cover.Height)}else{$null});candidates=@();chosen_owner=$null;chosen_point=$null;provider_limitation=$null;native_point_guard='RowPointerDriver foreground and Win32 point/process ownership'}
    foreach($fraction in @(@(.5,.5),@(.8,.8),@(.2,.8),@(.8,.2))){
        $at=@([int]($bounds.X+$bounds.Width*$fraction[0]),[int]($bounds.Y+$bounds.Height*$fraction[1]))
        $path=Hit-Path $at
        $owner=@($path|Where-Object {$_.process -eq $review.Id -and $_.id -eq $trace.expected_id -and $_.runtime_id -eq $expected})[0]
        $inside=Circle-Contains $bounds $at 2
        $covered=$cover -and (Circle-Contains $cover $at)
        $trace.candidates+=@{point=$at;fraction=$fraction;inside_requested_circle=$inside;covered_by_front_circle=[bool]$covered;ancestors=$path;uia_expected_owner=$owner}
        if(!$trace.chosen_point -and $inside -and !$covered){
            $trace.chosen_point=$at
            $trace.chosen_owner=@{id=$trace.expected_id;runtime_id=$expected;process=$button.Current.ProcessId;bounds=$trace.expected_bounds;source='Arranged swatch circle outside the shared front swatch'}
        }
    }
    if(!@($trace.candidates|Where-Object uia_expected_owner).Count){$trace.provider_limitation='FromPoint exposes the owned content bridge/window, without the arranged XAML swatch peer; raw ancestry is diagnostic'}
    $file=if($Receipt){$Receipt+'.json'}else{"slot-hit-$($script:slotMenuCases.Count)-$Device-$Slot.json"}
    [IO.File]::WriteAllText((Join-Path $run $file),($trace|ConvertTo-Json -Depth 12))
    $point=$trace.chosen_point
    if(!$point){throw "No exposed native point targets the $Slot swatch"}
    $trace
}
function Slot-OpenMenu([string]$Device,[string]$Slot){
    $before=Slot-Colors;$trace=Slot-Hit $Device $Slot;$point=$trace.chosen_point
    $button=Control ('color-'+$Slot) -Arranged
    if($Device -eq 'mouse'){[CapyRowPointer]::RightClick($point[0],$point[1])}
    elseif($Device -eq 'keyboard'){$button.SetFocus();Wait-Until {$button.Current.HasKeyboardFocus} 'The swatch did not take keyboard focus';[CapyRowPointer]::Key([uint32]$review.Id,0x5D)}
    else{
        [CapyRowPointer]::Down($Device,$point[0],$point[1])
        try{Start-Sleep -Milliseconds 1100;$trace.menu_before_release=[bool](Find 'color-swatch-edit' -Visible);$trace.colors_before_release=Slot-Colors}finally{[CapyRowPointer]::Up()}
        $trace.menu_after_release=[bool](Find 'color-swatch-edit' -Visible);$trace.colors_after_release=Slot-Colors
        [IO.File]::WriteAllText((Join-Path $run ("slot-hit-$($script:slotMenuCases.Count)-$Device-$Slot.json")),($trace|ConvertTo-Json -Depth 12))
    }
    $messages=[ordered]@{edit=@('common.ftl','native-color-edit-menu');palettes=@('common.ftl','native-color-palettes');swap=@('commands.ftl','commands-swap-foreground-and-background')}
    $items=@()
    foreach($action in $messages.Keys){
        $source=[IO.File]::ReadAllText((Join-Path $repo ('assets/locales/en/'+$messages[$action][0])))
        $caption=[regex]::Match($source,('(?m)^'+[regex]::Escape($messages[$action][1])+' = ([^\r\n]+)')).Groups[1].Value
        if(!$caption -or $caption.Contains('{')){throw 'The fixed English slot-menu caption is not a plain shared message'}
        $id='color-swatch-'+$action;$native=Control $id -Arranged
        if($native.Current.Name -ne $caption -or !$native.Current.IsEnabled){throw "The native $Device $Slot menu differs from the shared $action command"}
        $items+=@{id=$id;label=$native.Current.Name;message=$messages[$action][1];enabled=$native.Current.IsEnabled}
    }
    $presenter=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent((Control 'color-swatch-edit'))
    $nativeItems=@($presenter.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))|Where-Object {!$_.Current.IsOffscreen})
    if((($nativeItems|ForEach-Object {$_.Current.AutomationId}) -join ',') -ne 'color-swatch-edit,color-swatch-palettes,color-swatch-swap'){throw 'The native paint menu does not contain exactly the three shared actions in order'}
    if(@($presenter.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Separator))|Where-Object {!$_.Current.IsOffscreen}).Count){throw 'The native paint menu added a local separator'}
    if((Slot-Colors) -ne $before){throw "$Device context opening selected or changed the $Slot paint"}
    $script:slotMenuCases.Add(@{device=$Device;slot=$Slot;point=$point;items=$items})
    [IO.File]::WriteAllText((Join-Path $run 'slot-menu-cases.json'),($script:slotMenuCases|ConvertTo-Json -Depth 8))
}
function Slot-Closed{
    Wait-Until {!(Find 'edit-color-apply' -Visible) -and !(Find 'color-swatch-edit' -Visible) -and !(Find 'color-swatch-palettes' -Visible) -and !(Find 'color-swatch-swap' -Visible)} 'The native slot menu or editor did not close'
}
function Slot-Editor([string]$Slot){
    $null=Control 'edit-color-apply' -Arranged
    $pick=Control 'edit-color-pick' -Arranged
    if(!$pick.Current.IsEnabled){throw 'Slot Edit Color lost its enabled native canvas picker'}
    $frame=Control 'edit-color-dialog-frame' -Arranged;$source=(Model).state.colors.($Slot)
    if($source.space -ne 'Srgb' -or $source.rgba[3] -ne 1){throw 'The slot-menu fixture requires opaque sRGB paints for its initial Hex oracle'}
    $expected='#'+(@(0..2|ForEach-Object {([int][Math]::Round(255*[double]$source.rgba[$_])).ToString('X2')}) -join '')
    $record=[ordered]@{slot=$Slot;source_color=$source;expected_hex=$expected;native_name=$null;initial_hex=$null;current_control=(Control 'edit-color-current' -Within $frame).Current.Name}
    try{
        Wait-Until {$record.native_name=(Control 'edit-color-hex' -Within $frame).Current.Name;$record.initial_hex=$record.native_name.Substring($record.native_name.LastIndexOf(' ')+1);$record.initial_hex -eq $expected} 'Initial Edit Color Hex does not match the requested paint slot'
    }finally{[IO.File]::WriteAllText((Join-Path $run ("slot-editor-initial-$($script:slotMenuCases.Count)-$Slot.json")),($record|ConvertTo-Json -Depth 8))}
}
function Slot-Intact([string]$Name,[string]$Checkpoint,[string]$Pixels){
    if((Slot-Checkpoint) -ne $Checkpoint){throw "$Name changed artwork or Undo/Redo history"}
    Slot-Park
    $observed=Wait-StablePixels {Slot-Pixels}
    Slot-Receipt ('slot-intact-'+$Name.Replace(' ','-')) $Pixels $observed
    if($observed -ne $Pixels){throw "$Name changed exact composed artwork pixels"}
}
function Slot-MenuJourney{
    $script:slotMenuCases=[Collections.Generic.List[object]]::new()
    Fit-Canvas
    $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$camera=(Model).state.camera;$area=$camera.work_area
    $script:slotPixelPoint=@([int]($canvas.X+($area[0]+$area[2]/2)*$canvas.Width/$camera.viewport[0]),[int]($canvas.Y+($area[1]+$area[3]/2)*$canvas.Height/$camera.viewport[1]))
    Slot-Park
    Wait-Until {$model=Model;$model.canvas_ready -and $model.brush_ready -and $model.shaders_ready} 'The slot-menu pixel baseline did not finish its idle shaders'
    $empty=Wait-StablePixels {Slot-Pixels};$before=Slot-Checkpoint
    Slot-Receipt 'slot-empty' $empty $empty
    [CapyRowPointer]::Down('mouse',($script:slotPixelPoint[0]-30),$script:slotPixelPoint[1])
    try{for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move(($script:slotPixelPoint[0]-30+$step*5),$script:slotPixelPoint[1]);Start-Sleep -Milliseconds 12}}finally{[CapyRowPointer]::Up()}
    Slot-Park
    Wait-Until {(Slot-Checkpoint) -ne $before -and ((Model).state.commands|Where-Object id -eq 'undo').enabled -and (Slot-Pixels) -ne $empty} 'The native slot-menu seed did not establish artwork and Undo history' 45
    $artwork=Wait-StablePixels {Slot-Pixels};$checkpoint=Slot-Checkpoint
    Slot-Receipt 'slot-seed' $artwork $artwork
    foreach($slot in @('background','foreground')){
        $colors=(Model).state.colors
        $paints=@($colors.foreground,$colors.background,$colors.temporary)|ConvertTo-Json -Depth 8 -Compress
        $trace=Slot-Hit 'touch' $slot ('slot-short-touch-hit-'+$slot);$point=$trace.chosen_point
        [CapyRowPointer]::Down('touch',$point[0],$point[1])
        try{Start-Sleep -Milliseconds 40}finally{[CapyRowPointer]::Up()}
        Wait-Until {$colors=(Model).state.colors;$colors.slot -eq $slot -and $colors.paint_slot -eq $slot} 'A short touch did not select its requested paint slot'
        if(Find 'color-swatch-edit' -Visible){throw 'A short touch opened the paint context menu'}
        $colors=(Model).state.colors
        if((@($colors.foreground,$colors.background,$colors.temporary)|ConvertTo-Json -Depth 8 -Compress) -ne $paints){throw 'A short touch changed the paint definitions'}
        Slot-Intact ('short touch '+$slot) $checkpoint $artwork
    }
    foreach($device in @('mouse','keyboard','pen','touch')){
        $active=if($device -in @('mouse','pen')){'transparent'}else{'foreground'}
        Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not select before the slot context'
        if($active -eq 'transparent'){Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before the slot context'}
        $colors=Slot-Colors
        Slot-OpenMenu $device 'background';Invoke 'color-swatch-edit'
        Slot-Editor 'background'
        if((Slot-Colors) -ne $colors){throw "$device Edit Color changed the active $active slot before Apply"}
        Pair-Value 'edit-color-hex' '#B53177'
        if((Slot-Colors) -ne $colors){throw "$device Edit Color published an unaccepted draft"}
        Capture ("slot-menu-$device-cancel") -WithModel -Composed
        Invoke 'edit-color-cancel';Slot-Closed
        Wait-Until {(Slot-Colors) -eq $colors} "$device Cancel did not preserve the exact colors and active $active slot"
        Slot-Intact "$device Cancel" $checkpoint $artwork
    }
    foreach($case in @(@{target='background';active='foreground';hex='#38A6C9'},@{target='foreground';active='background';hex='#C94E38'})){
        Invoke ('color-'+$case.active);Wait-Until {(Model).state.colors.slot -eq $case.active} 'The other paint did not select before Apply'
        $other=(Model).state.colors.($case.active)|ConvertTo-Json -Compress
        Slot-OpenMenu 'mouse' $case.target;Invoke 'color-swatch-edit';Slot-Editor $case.target
        Pair-Value 'edit-color-hex' $case.hex;Invoke 'edit-color-apply';Slot-Closed
        $expected=@(1,3,5|ForEach-Object {[Convert]::ToInt32($case.hex.Substring($_,2),16)/255.})
        Wait-Until {$colors=(Model).state.colors;$color=$colors.($case.target);$colors.slot -eq $case.target -and $colors.paint_slot -eq $case.target -and $color.space -eq 'Srgb' -and [Math]::Abs($color.rgba[0]-$expected[0]) -lt .000001 -and [Math]::Abs($color.rgba[1]-$expected[1]) -lt .000001 -and [Math]::Abs($color.rgba[2]-$expected[2]) -lt .000001 -and $color.rgba[3] -eq 1} 'Apply did not edit and select the requested paint slot'
        if(((Model).state.colors.($case.active)|ConvertTo-Json -Compress) -ne $other){throw 'Apply changed the other paint slot'}
        Slot-Intact ('Apply '+$case.target) $checkpoint $artwork
        Capture ('slot-menu-apply-'+$case.target) -WithModel -Composed
    }
    $colors=Slot-Colors
    $undoRevision=(Model).state.document_file.revision
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5A);Slot-Park
    Wait-Until {(Model).state.document_file.revision -gt $undoRevision -and !((Model).state.commands|Where-Object id -eq 'undo').enabled -and ((Model).state.commands|Where-Object id -eq 'redo').enabled} 'One Undo after slot menus did not acknowledge removing the seed stroke'
    $undone=Wait-StablePixels {Slot-Pixels}
    Slot-Receipt 'slot-undone' $empty $undone
    if($undone -ne $empty){throw 'One Undo after slot menus did not remove only the seed stroke'}
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x59);Slot-Park
    Wait-Until {(Slot-Pixels) -eq $artwork} 'One Redo after slot menus did not restore exact seed artwork'
    if((Slot-Colors) -ne $colors){throw 'Artwork Undo/Redo changed the accepted slot paints'}
    $checkpoint=Slot-Checkpoint
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not select before Palettes'
    $foreground=(Model).state.colors.foreground|ConvertTo-Json -Compress
    Slot-OpenMenu 'keyboard' 'background';Invoke 'color-swatch-palettes'
    Wait-Until {(Model).state.colors.slot -eq 'background' -and (Model).state.colors.paint_slot -eq 'background' -and @((Model).layout.groups|Where-Object active -eq 'palettes').Count -gt 0 -and (Find 'palettes-panel' -Visible)} 'Palettes did not select the requested slot and reveal its native panel'
    if(Find 'edit-color-apply' -Visible){throw 'Palettes opened an Edit Color dialog'}
    $swatch=@((Model).palette_panel.swatches|Where-Object {!$_.current})[0]
    if(!$swatch){throw 'The shared Palettes view has no different paint to select'}
    Invoke ('palette-swatch-'+$swatch.id)
    Wait-Until {(Control ('palette-swatch-'+$swatch.id)).Current.ItemStatus -eq 'Selected' -and (((Model).state.colors.background|ConvertTo-Json -Compress) -eq ($swatch.color|ConvertTo-Json -Compress))} 'The native palette swatch did not edit the requested background paint'
    if(((Model).state.colors.foreground|ConvertTo-Json -Compress) -ne $foreground -or (Slot-Checkpoint) -ne $checkpoint){throw 'Palettes changed the other paint, artwork or history'}
    Capture 'slot-menu-palettes' -WithModel -Composed
    Slot-ColorPanel
    Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before Swap'
    $foreground=(Model).state.colors.foreground|ConvertTo-Json -Compress;$background=(Model).state.colors.background|ConvertTo-Json -Compress;$paintSlot=(Model).state.colors.paint_slot
    Slot-Park;$pixels=Wait-StablePixels {Slot-Pixels}
    Slot-OpenMenu 'mouse' 'background';Invoke 'color-swatch-swap';Slot-Closed
    Wait-Until {$colors=(Model).state.colors;$colors.slot -eq 'transparent' -and $colors.paint_slot -eq $paintSlot -and ($colors.foreground|ConvertTo-Json -Compress) -eq $background -and ($colors.background|ConvertTo-Json -Compress) -eq $foreground} 'The shared Swap menu action did not preserve Transparent and exchange the exact paints'
    Slot-Intact 'Swap' $checkpoint $pixels
    Capture 'slot-menu-swap' -WithModel -Composed
}
function Pair-Journey{
    Pair-Setup
    $document=(Model).state.document_file|ConvertTo-Json -Compress
    foreach($slot in @('background','foreground')){
        Invoke "color-$slot";Wait-Until {(Model).paint_pair.front_swatch -eq $slot} 'Paint selection did not reach the shared icon view'
        Assert-Pair "pair-$slot"
        Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select for icon memory'
        Assert-Pair "pair-$slot-transparent"
    }
    foreach($name in @('Black','White')){
        Quick-Color $name;Wait-Until {(Model).state.colors.slot -eq 'temporary'} "$name did not enter Temporary"
        Assert-Pair "pair-temporary-$($name.ToLower())"
    }
    if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $document){throw 'Icon selection changed artwork history'}
    (Control 'panel-tab-sizes').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
    $panel=@((Model).panels|Where-Object id -eq 'sizes')[0];Invoke ($panel.configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq 'sizes'} 'Brush configuration did not open'
    Scroll-Position (Control 'configure-show-brush_size' -Arranged) 100
    $preview=Control 'configure-control-brush_color' -Arranged;Pair-Park;Capture 'pair-temporary-active-preview' -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run 'pair-temporary-active-preview.png'))
    try{
        $bounds=$preview.Current.BoundingRectangle;$actual=Swatch-Pixel $bitmap @([int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
        if(@($actual|Where-Object {$_ -lt 247}).Count){throw 'Temporary white did not update the actual brush preview'}
    }finally{$bitmap.Dispose()}
    (Control 'panel-tab-sizes').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Model).state.customization.expanded} 'Brush configuration did not close'
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not restore'
    Pair-SetPaint @('0.1','0.35','0.6');Assert-Pair 'pair-foreground-edited'
    Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'Background did not select before its edit'
    Pair-SetPaint @('0.8','0.1','0.3');Assert-Pair 'pair-background-edited'
    Pair-SetPaint @('1','1','1');Invoke 'color-foreground'
    Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not select before mask entry'
    Assert-Pair 'pair-artwork-before-mask'
    $artwork=(Model).state.colors|ConvertTo-Json -Depth 12 -Compress
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x51)
    Wait-Until {(Model).state.layer_tools.mask_editing} 'Quick Mask did not publish its paints'
    Invoke 'color-background';Wait-Until {(Model).paint_pair.front_swatch -eq 'background'} 'Mask background selection did not publish'
    Pair-SetPaint @('0.2','0.2','0.2')
    if(((Model).state.layer_tools.mask_editing.colors.background|ConvertTo-Json -Compress) -eq ((Model).state.colors.background|ConvertTo-Json -Compress)){throw 'Mask icon fixture did not establish a separate paint definition'}
    Assert-Pair 'pair-mask-background'
    Invoke 'color-transparent';Wait-Until {@((Model).color_panel.swatches|Where-Object {$_.slot -eq 'transparent' -and $_.selected}).Count -eq 1} 'Mask Transparent did not select';Assert-Pair 'pair-mask-transparent'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Model).state.layer_tools.mask_editing} 'Quick Mask did not leave'
    if(((Model).state.colors|ConvertTo-Json -Depth 12 -Compress) -ne $artwork){throw 'Mask paint selection changed artwork paints'}
    Assert-Pair 'pair-artwork-restored'
    Pair-Command 'new_document' 'File';New-DrawingPixels 128 96
    Pair-Choice 'document-depth' '16-bit float HDR';Invoke 'Create' -Name
    Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and !(Model).state.document_file.busy} 'Float drawing did not open' 60
    Pair-SetPaint @('1','0.5','0.25') '2'
    if((Model).paint_pair.definition.rgba[0] -le 1){throw 'HDR icon fixture did not establish paint above SDR white'}
    Assert-Pair 'pair-hdr'
    $definition=(Model).paint_pair.definition|ConvertTo-Json -Compress;$rgba=(Model).paint_pair.rgba|ConvertTo-Json -Compress
    if(Find 'panel-tab-proof'){Invoke 'panel-tab-proof'}else{Invoke 'column-icon-proof'}
    Wait-Until {Find 'proof-panel-mode-sdr'} 'HDR Proof controls did not open'
    Invoke 'proof-panel-mode-sdr';Wait-Until {(Model).windows_proof_form.mode -eq 'sdr'} 'Proof panel did not enter SDR mode'
    (Reveal-PanelControl 'proof' 'proof-panel-exposure').SetFocus();Pair-Text 'proof-panel-exposure' '-50';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    $updated=@{pair=$null}
    Wait-Until {$updated.pair=(Model).paint_pair;$updated.pair -and (($updated.pair.rgba|ConvertTo-Json -Compress) -ne $rgba)} 'SDR rendition did not change the shared icon preview'
    if(($updated.pair.definition|ConvertTo-Json -Compress) -ne $definition){throw 'SDR rendition changed stored paint'}
    Assert-Pair 'pair-hdr-rendition'
}

function Swatch-Journey{
    $document=(Model).state.document_file|ConvertTo-Json -Compress
    $ids=@('color-foreground','color-background','color-transparent')
    $retained=@{};foreach($id in $ids){$retained[$id]=(Control $id).GetRuntimeId() -join ':'}
    foreach($device in @('mouse','pen','touch')){
        foreach($slot in @('background','foreground')){
            Swatch-Tap $device (Swatch-Point "color-$slot")
            Wait-Until {(Model).state.colors.slot -eq $slot} "$device did not select $slot"
            Park-Pointer;Assert-Front $slot "$device-$slot-front"
            Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before overlap input'
            Swatch-Tap $device (Overlap-Point)
            Wait-Until {(Model).state.colors.slot -eq $slot} "$device overlap selected the rear paint instead of $slot"
            Write-Output "Native $device overlap selected $slot"
            Park-Pointer;Assert-Rim "color-$slot" "$device-$slot-selected"
            $rear=if($slot -eq 'foreground'){'background'}else{'foreground'}
            $at=Swatch-Point "color-$rear";[CapyRowPointer]::Hover($at[0],$at[1])
            Assert-Rim "color-$rear" "$device-$rear-hover";Assert-Front $slot "$device-$slot-hover-front"
            Park-Pointer;[CapyRowPointer]::PenHover($at[0],$at[1])
            Assert-Rim "color-$rear" "$device-$rear-pen-hover";Assert-Front $slot "$device-$slot-pen-hover-front"
            [CapyRowPointer]::PenLeave()
            Swatch-Tap $device (Swatch-Point 'color-transparent')
            Wait-Until {(Model).state.colors.slot -eq 'transparent'} "$device did not select transparent"
            Park-Pointer;Assert-Front $slot "$device-$slot-transparent-memory";Assert-Rim 'color-transparent' "$device-$slot-transparent-selected"
            Swatch-Tap $device (Overlap-Point)
            Wait-Until {(Model).state.colors.slot -eq $slot} "$device overlap selected the rear paint instead of $slot"
        }
    }
    foreach($slot in @('background','foreground')){
        Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before keyboard input'
        $button=Control "color-$slot";Focus-Keyboard $button
        [CapyRowPointer]::Key([uint32]$review.Id,0x20)
        Wait-Until {(Model).state.colors.slot -eq $slot} "Keyboard did not select $slot"
        if(!$button.Current.HasKeyboardFocus){throw 'Raising the focused swatch lost keyboard focus'}
        Park-Pointer;Assert-Front $slot "keyboard-$slot-front"
    }
    Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before the corner tap'
    Park-Pointer
    $bounds=(Control 'color-foreground' -Arranged).Current.BoundingRectangle
    $corner=@([int]($bounds.X+1),[int]($bounds.Y+1))
    Swatch-Tap 'mouse' $corner
    if((Model).state.colors.slot -ne 'transparent'){throw 'Swatch square corner changed the paint slot'}
    foreach($id in $ids){if(((Control $id).GetRuntimeId() -join ':') -ne $retained[$id]){throw "Swatch feedback replaced retained control $id"}}
    if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $document){throw 'Swatch input changed the document'}
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not restore after swatch journey'
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$theme;language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $CapyTraceDirectory=$run
    # The smoke command strip covers the bottom swatches at this window size.
    # This fixture uses native controls only; keep the isolated profile and all
    # foreground/point ownership checks, without the unrelated overlay.
    Remove-Item Env:CAPY_SMOKE_TEST -ErrorAction SilentlyContinue
    $workingDirectory=$run
    $review=Start-Process -FilePath $Executable -WorkingDirectory $workingDirectory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $run 'pid.txt'),[string]$review.Id)
    Write-Output "Compact color review $($review.Id), $run"
    $owned=@{window=$null}
    Wait-Until {$m=Model;if(!$m.brush_ready -or !$m.windows_workspace.ready){return $false};$owned.window=Owned-DrawingWindow $review -TraceDirectory $run;$null -ne $owned.window} 'Color fixture did not start' 60
    $root=$owned.window.Root
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow([IntPtr]$root.Current.NativeWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace
    $panel=(Control 'color-panel').Current.BoundingRectangle
    foreach($id in @('color-background','color-foreground','color-transparent','color-swap','color-shape-0','color-shape-1','color-readout')){
        $bounds=(Control $id).Current.BoundingRectangle
        if($bounds.Left -lt $panel.Left-1 -or $bounds.Top -lt $panel.Top-1 -or $bounds.Right -gt $panel.Right+1 -or $bounds.Bottom -gt $panel.Bottom+1){
            throw "Compact color control is clipped: $id"
        }
    }
    if($slotMenuOnly){Slot-MenuJourney}
    if(!$slotMenuOnly -and $Journey -notin @('pair-dark','pair-light')){Swatch-Journey}
    if($Journey -eq 'full'){
    $document=(Model).state.document_file|ConvertTo-Json -Compress
    $retained=(Control 'color-shape-0').GetRuntimeId() -join ':'
    foreach($shape in @('circle','square','triangle')){
        Shape $shape
        $mode=(Model).color_panel.readout
        $paint=Paint;Invoke 'color-readout'
        Wait-Until {(Model).color_panel.readout -ne $mode} 'Readout did not toggle'
        if((Paint) -ne $paint){throw 'Readout toggle changed paint'}
        Invoke 'color-readout'
        Wait-Until {(Model).color_panel.readout -eq $mode} 'Readout did not restore'
        $deviceIndex=0
        foreach($device in @('mouse','pen','touch')){
            Pick-Field $device (.56+.02*$deviceIndex) (.46-.01*$deviceIndex)
            # This ring point is beneath the readout's bounding box. Its curved
            # native hit area must leave the hue ring available for every device.
            $hue=(Model).color_panel.wheel_components[0];$angle=@(-135,-120,-150)[$deviceIndex]*[Math]::PI/180
            $ring=Point (.5+.455*[Math]::Cos($angle)) (.5+.455*[Math]::Sin($angle))
            [CapyRowPointer]::Down($device,$ring[0],$ring[1]);[CapyRowPointer]::Up()
            Wait-Until {[Math]::Abs((Model).color_panel.wheel_components[0]-$hue) -gt 1} "$device readout intercepted the $shape hue ring"
            $start=Point .5 .5
            [CapyRowPointer]::Down($device,$start[0],$start[1]);[CapyRowPointer]::Cancel();[CapyRowPointer]::Verify();$deviceIndex++
        }
        Capture $shape
    }
    if(((Control 'color-shape-0').GetRuntimeId() -join ':') -ne $retained){throw 'Color changes replaced retained shape controls'}
    $button=Control 'color-shape-0';$target=(Model).color_panel.other_shapes[0];Focus-Keyboard $button
    $focused=[System.Windows.Automation.AutomationElement]::FocusedElement
    @{process_id=$review.Id;target=$button.Current.AutomationId;focused_process=$focused.Current.ProcessId;focused_id=$focused.Current.AutomationId;has_keyboard_focus=$button.Current.HasKeyboardFocus;shape=(Model).color_panel.shape;requested_shape=$target}|ConvertTo-Json|Set-Content (Join-Path $run 'shape-keyboard-focus.json')
    [CapyRowPointer]::Key([uint32]$review.Id,0x20)
    Wait-Until {(Model).color_panel.shape -eq $target} 'Space did not activate the focused native shape button'
    Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'Background selection failed'
    Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent selection failed'
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground selection failed'
    $fg=(Model).state.colors.foreground|ConvertTo-Json -Compress
    $bg=(Model).state.colors.background|ConvertTo-Json -Compress
    Invoke 'color-swap'
    Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Compress) -eq $bg -and ((Model).state.colors.background|ConvertTo-Json -Compress) -eq $fg} 'Color swap lost paint'
    Invoke 'color-swap'
    Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Compress) -eq $fg} 'Color swap did not restore paint'
    foreach($device in @('mouse','pen','touch','keyboard')){
        $button=Control 'color-foreground';$bounds=$button.Current.BoundingRectangle
        $x=[int]($bounds.X+$bounds.Width*.5);$y=[int]($bounds.Y+$bounds.Height*.5)
        if($device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y)}
        elseif($device -eq 'keyboard'){$button.SetFocus();[CapyRowPointer]::Key(0x5D)}
        else{[CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 1100;[CapyRowPointer]::Up()}
        Wait-Until {$null -ne (Find 'color-swatch-swap')} "$device did not open the native paint menu"
        [CapyRowPointer]::Key(0x1B)
        Wait-Until {$null -eq (Find 'color-swatch-swap')} "$device paint menu did not dismiss"
    }
    $bounds=(Control 'color-foreground').Current.BoundingRectangle
    [CapyRowPointer]::Down('mouse',[int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.5))
    Start-Sleep -Milliseconds 1100
    if(Find 'color-swatch-swap'){throw 'Mouse hold incorrectly opened the paint menu'}
    [CapyRowPointer]::Up()
    # Partial Zen was retired in the shared UI. Exercise the retained drawer
    # through its ordinary toolbar tile, which is the current user workflow.
    $tileId=((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'}|Select-Object -ExpandProperty id
    Invoke "tile-toolbar-$tileId"
    Wait-Until {$null -ne (Find 'tool-drawer')} 'Color drawer did not open'
    $windowRoot=$root;$root=Control 'tool-drawer'
    # A visible drawer can still be moving from its opening animation.
    # Require the complete wheel to stay at its final coordinates before input.
    $settled=@{bounds='';count=0}
    Wait-Until {
        $wheel=Find 'color-wheel';if(!$wheel){return $false}
        $bounds=$wheel.Current.BoundingRectangle
        $current=$bounds.ToString()
        if($bounds.Width -gt 0 -and [Math]::Abs($bounds.Width-$bounds.Height) -lt 1 -and $current -eq $settled.bounds){$settled.count++}else{$settled.count=0}
        $settled.bounds=$current
        $settled.count -ge 3
    } 'Color drawer wheel did not finish its opening animation'
    $drawerButton=(Control 'color-shape-0').GetRuntimeId() -join ':'
    Shape circle
    $index=0
    foreach($device in @('mouse','pen','touch')){
        Pick-Field $device (.54+.04*$index) .56
        $index++
    }
    Invoke 'color-readout'
    if(((Control 'color-shape-0').GetRuntimeId() -join ':') -ne $drawerButton){throw 'Drawer edits replaced retained color controls'}
    Capture 'retained-drawer'
    $root=$windowRoot
    Invoke "tile-toolbar-$tileId"
    Wait-Until {$null -eq (Find 'tool-drawer')} 'Color drawer did not close'
    if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $document){throw 'Picker input painted or changed the document'}
    }
    if($Journey -in @('full','pair-dark','pair-light')){Pair-Journey}
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $root.Current.NativeWindowHandle -StateDirectory $workingDirectory -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native color stderr requires inspection'}
    $result=[ordered]@{
        theme=$theme;compact_bounds='passed'
        scope='Composed UI pixels and synthetic native input; physical digitizer and hardware performance acceptance remain separate'
    }
    if(!$slotMenuOnly -and $Journey -notin @('pair-dark','pair-light')){
        foreach($check in @('swatch_overlap_and_transparent_memory','retained_swatch_focus','circular_swatch_hits','keyboard_activation','document_unchanged')){$result[$check]='passed'}
        if($Journey -ne 'input'){$result['selected_and_hover_rims']='passed'}
    }
    if($Journey -in @('full','pair-dark','pair-light')){$result['retained_header_toolbar_and_tab_pair_pixels']='passed';$result['temporary_active_preview']='passed';$result['mask_alpha_and_rendition']='passed'}
    if($slotMenuOnly){$result['slot_menu_native_context_devices']='mouse, keyboard, pen, touch';$result['slot_menu_exact_shared_captions']='passed';$result['other_slot_cancel_and_apply']='passed';$result['palettes_select_reveal_and_paint']='passed';$result['swap_while_transparent']='passed';$result['artwork_pixels_and_history']='exact';$result['evidence']=$run}
    if($Journey -eq 'full'){
        foreach($check in @('shapes_and_readouts','mouse_pen_touch_fields_and_ring','cancellation','retained_controls','paint_slots_and_swap','native_context_menus','mouse_hold_no_menu','retained_drawer_input')){$result[$check]='passed'}
    }
    [IO.File]::WriteAllText((Join-Path $run 'results.json'),($result|ConvertTo-Json -Depth 8))
    $result|ConvertTo-Json -Depth 8
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel -Composed}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
