param([Parameter(Mandatory)][string]$Executable,[ValidateSet('full','dark','light','input','pair-dark','pair-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
$CapyFind='visible'
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing,System.Windows.Forms
$CapyCaptureDelay=250
$theme=if($Journey -in @('light','pair-light')){'light'}else{'dark'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
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
function Hit-Id($At){
    $hit=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($At[0],$At[1]))
    if($hit.Current.ProcessId -ne $review.Id){throw 'Swatch hit is outside the owned review'}
    while($hit){
        if($hit.Current.AutomationId -like 'color-*'){return $hit.Current.AutomationId}
        $hit=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($hit)
    }
}
function Circle-Contains($Bounds,$At){
    $dx=$At[0]-$Bounds.X-$Bounds.Width/2;$dy=$At[1]-$Bounds.Y-$Bounds.Height/2
    $dx*$dx+$dy*$dy -le [Math]::Pow($Bounds.Width/2+1,2)
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
    if(![CapyRowPointer]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Swatch capture origin is unavailable'}
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
        $rgba=($snapshot.color_panel.swatches|Where-Object slot -eq $Slot).rgba
        for($i=0;$i -lt 3;$i++){if([Math]::Abs($actual[$i]-$rgba[$i]*255) -gt 8){throw "$Name overlap shows $($actual -join ',') instead of $Slot paint"}}
    }finally{$bitmap.Dispose()}
    @{point=$at;ui_automation=(Hit-Id $at);front=$Slot}|ConvertTo-Json|Set-Content (Join-Path $run ($Name+'-hit.json'))
}
function Assert-Rim([string]$Id,[string]$Name){
    if($Journey -eq 'input'){return}
    $bounds=(Control $Id -Arranged).Current.BoundingRectangle
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
    $snapshot=Model
    $front='color-'+$snapshot.color_panel.front_swatch
    $order=@('color-white','color-black','color-background','color-foreground','color-transparent')|Where-Object {$_ -ne $front}
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
            $best=$sector|Sort-Object difference|Select-Object -First 1;$samples+=,@{angle=$angle;pixels=$sector}
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
    @(@{kind='header';button=$button;size=$size;labels=0},@{kind='toolbar';button=(Control ('tile-toolbar-'+$tile.id) -Arranged);size=$geometry.tile_icon_size;labels=$geometry.tile_label_lines})
}
function Pair-Setup{
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1800 -Height 1300
    Pair-Command 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Header editor did not open for color icon fixture'
    $from=(Control 'header-component-tools' -Arranged).Current.BoundingRectangle
    $presentation=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json;$zone=$presentation.geometry.zones[1]
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Header fixture origin is unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
    [CapyRowPointer]::Down('mouse',[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
    [CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width/2)*$scale),[int]($origin.y+($zone.y+$zone.height/2)*$scale))
    Wait-Until {try{$gesture=(Control 'title-bar').Current.HelpText|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $gesture.preview.target}catch{$false}} 'Header color insertion has no target'
    [CapyRowPointer]::Up();Wait-Until {(Model).picker} 'Header color picker did not open'
    Pair-Text 'tool-picker-search' 'choose current paint color'
    Wait-Until {Find 'picker-choice-color-0'} 'Header picker omitted Color'
    (Control 'picker-choice-color-0').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Invoke 'Add Tools' -Name
    Wait-Until {!(Model).picker} 'Header color picker did not close'
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Header color editor did not finish'
    $script:pairIdentities=@{}
    foreach($control in (Pair-Controls)){$script:pairIdentities[$control.kind]=$control.button.GetRuntimeId() -join ':'}
}
function Assert-Pair([string]$Name){
    Pair-Park;$snapshot=Model;$pair=$snapshot.paint_pair
    if(!$pair -or $pair.swatches.Count -ne 2){throw 'Shared paint pair view is missing'}
    $controls=Pair-Controls;Capture $Name -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')));$samples=@()
    try{
        foreach($control in $controls){
            if(($control.button.GetRuntimeId() -join ':') -ne $script:pairIdentities[$control.kind]){throw "$Name replaced retained $($control.kind) color button"}
            $bounds=$control.button.Current.BoundingRectangle
            $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.;$unit=$control.size*$scale/16.
            if($unit -le 0){throw "Missing canonical icon size for $($control.kind)"}
            $left=$bounds.X+$bounds.Width/2-8*$unit
            if($control.labels -gt 0){$left=$bounds.X+18*$scale-8*$unit}
            $top=$bounds.Y+$bounds.Height/2-8*$unit
            foreach($slot in @('foreground','background')){
                $swatch=@($pair.swatches|Where-Object slot -eq $slot)[0]
                $center=if($slot -eq 'foreground'){6.75}else{11.};$radius=if($slot -eq 'foreground'){6.}else{4.25}
                $otherCenter=if($slot -eq 'foreground'){11.}else{6.75};$otherRadius=if($slot -eq 'foreground'){4.25}else{6.}
                $seen=@{};$count=0
                for($y=0;$y -lt 16;$y++){
                    for($x=0;$x -lt 16;$x++){
                        $at=@([int][Math]::Floor($left+($x+.5)*$unit),[int][Math]::Floor($top+($y+.5)*$unit))
                        $cx=($at[0]+.5-$left)/$unit;$cy=($at[1]+.5-$top)/$unit
                        if([Math]::Sqrt([Math]::Pow($cx-$center,2)+[Math]::Pow($cy-$center,2)) -ge $radius-1.5){continue}
                        if($slot -ne $pair.front_swatch -and [Math]::Sqrt([Math]::Pow($cx-$otherCenter,2)+[Math]::Pow($cy-$otherCenter,2)) -le $otherRadius+1){continue}
                        if($swatch.rgba[3] -lt 1){
                            $cell=$pair.checker_cell;$dx=($cx-$center+$radius)%$cell;$dy=($cy-$center+$radius)%$cell
                            if([Math]::Min($dx,$cell-$dx)*$unit -lt 1.5 -or [Math]::Min($dy,$cell-$dy)*$unit -lt 1.5){continue}
                        }
                        $pixel=Swatch-Pixel $bitmap $at;$best=255;$which=-1
                        for($i=0;$i -lt 2;$i++){
                            $difference=0;for($channel=0;$channel -lt 3;$channel++){$difference=[Math]::Max($difference,[Math]::Abs($pixel[$channel]-255*$swatch.checker[$i][$channel]))}
                            if($difference -lt $best){$best=$difference;$which=$i}
                        }
                        $samples+=@{control=$control.kind;slot=$slot;point=$at;canonical=@($cx,$cy);origin=@($left,$top);unit=$unit;rgb=$pixel;difference=$best}
                        if($best -gt 8){throw "$Name $($control.kind) $slot glyph shows $($pixel -join ',') outside its shared opaque checker colors"}
                        $seen[$which]=$true;$count++
                    }
                }
                if($count -lt 6){throw "$Name has too few visible $slot icon samples"}
                if([Math]::Abs($swatch.checker[0][0]-$swatch.checker[1][0])*255 -gt 16 -and $seen.Count -ne 2){throw "$Name $slot glyph omitted a shared checker color"}
            }
        }
    }finally{$bitmap.Dispose();$samples|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ($Name+'-pixels.json'))}
}
function Pair-SetPaint([string[]]$Channels,[string]$Alpha='100',[string]$Intensity=''){
    $before=(Model).paint_pair.definition|ConvertTo-Json -Compress
    Invoke 'color-edit';Wait-Until {Find 'precise-color-model'} 'Precise paint editor did not open'
    Pair-Choice 'precise-color-model' 'Linear RGB'
    for($i=0;$i -lt 3;$i++){Pair-Text "precise-color-$i" $Channels[$i]}
    Pair-Text 'precise-color-3' $Alpha
    if($Intensity){Pair-Text 'precise-color-intensity' $Intensity}
    Invoke 'precise-color-apply'
    Wait-Until {$pair=(Model).paint_pair;$pair -and $pair.definition.rgba -and (($pair.definition|ConvertTo-Json -Compress) -ne $before) -and $pair.definition.rgba[3] -eq [double]$Alpha/100} 'Paint definition did not publish'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B);Wait-Until {!(Find 'precise-color-apply')} 'Paint editor did not close'
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
    foreach($name in @('black','white')){
        Invoke "color-$name";Wait-Until {(Model).state.colors.slot -eq 'temporary'} 'Quick color did not enter Temporary'
        Assert-Pair "pair-temporary-$name"
    }
    if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $document){throw 'Icon selection changed artwork history'}
    (Control 'panel-tab-sizes').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
    $panel=@((Model).panels|Where-Object id -eq 'sizes')[0];Invoke ($panel.configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq 'sizes'} 'Brush configuration did not open'
    $preview=Control 'configure-control-brush_color';Scroll-Position $preview 100
    $preview=Control 'configure-control-brush_color' -Arranged;Pair-Park;Capture 'pair-temporary-active-preview' -Composed -WithModel
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run 'pair-temporary-active-preview.png'))
    try{
        $bounds=$preview.Current.BoundingRectangle;$actual=Swatch-Pixel $bitmap @([int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
        if(@($actual|Where-Object {$_ -lt 247}).Count){throw 'Temporary white did not update the actual brush preview'}
    }finally{$bitmap.Dispose()}
    (Control 'panel-tab-sizes').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Model).state.customization.expanded} 'Brush configuration did not close'
    Invoke 'color-foreground';Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not restore'
    foreach($alpha in @('50','0')){Pair-SetPaint @('0.1','0.35','0.6') $alpha;Assert-Pair "pair-alpha-$alpha"}
    Pair-SetPaint @('0.1','0.35','0.6') '100'
    Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'Background did not select before alpha edit'
    Pair-SetPaint @('0.8','0.1','0.3') '50';Assert-Pair 'pair-background-alpha'
    Pair-SetPaint @('1','1','1') '100';Invoke 'color-foreground'
    Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground did not select before mask entry'
    Assert-Pair 'pair-artwork-before-mask'
    $artwork=(Model).state.colors|ConvertTo-Json -Depth 12 -Compress
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x51)
    Wait-Until {(Model).state.layer_tools.mask_editing} 'Quick Mask did not publish its paints'
    Invoke 'color-background';Wait-Until {(Model).paint_pair.front_swatch -eq 'background'} 'Mask background selection did not publish'
    Pair-SetPaint @('0.2','0.2','0.2') '100'
    if(((Model).state.layer_tools.mask_editing.colors.background|ConvertTo-Json -Compress) -eq ((Model).state.colors.background|ConvertTo-Json -Compress)){throw 'Mask icon fixture did not establish a separate paint definition'}
    Assert-Pair 'pair-mask-background'
    Invoke 'color-transparent';Wait-Until {@((Model).color_panel.swatches|Where-Object {$_.slot -eq 'transparent' -and $_.selected}).Count -eq 1} 'Mask Transparent did not select';Assert-Pair 'pair-mask-transparent'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Model).state.layer_tools.mask_editing} 'Quick Mask did not leave'
    if(((Model).state.colors|ConvertTo-Json -Depth 12 -Compress) -ne $artwork){throw 'Mask paint selection changed artwork paints'}
    Assert-Pair 'pair-artwork-restored'
    Pair-Command 'new_document' 'File';Pair-Text 'document-width' '128';Pair-Text 'document-height' '96'
    Pair-Choice 'document-depth' '16-bit float HDR';Invoke 'Create' -Name
    Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and !(Model).state.document_file.busy} 'Float drawing did not open' 60
    Pair-SetPaint @('1','0.5','0.25') '100' '2'
    if((Model).paint_pair.definition.rgba[0] -le 1){throw 'HDR icon fixture did not establish paint above SDR white'}
    Assert-Pair 'pair-hdr'
    $definition=(Model).paint_pair.definition|ConvertTo-Json -Compress;$rgba=(Model).paint_pair.rgba|ConvertTo-Json -Compress
    if(Find 'panel-tab-proof'){Invoke 'panel-tab-proof'}else{Invoke 'column-icon-proof'}
    Wait-Until {Find 'proof-panel-mode-sdr'} 'HDR Proof controls did not open'
    Invoke 'proof-panel-mode-sdr';Wait-Until {Find 'proof-panel-exposure'} 'SDR appearance controls did not open'
    (Control 'proof-panel-exposure').SetFocus();Pair-Text 'proof-panel-exposure' '-50';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    $updated=@{pair=$null}
    Wait-Until {$updated.pair=(Model).paint_pair;$updated.pair -and (($updated.pair.rgba|ConvertTo-Json -Compress) -ne $rgba)} 'SDR rendition did not change the shared icon preview'
    if(($updated.pair.definition|ConvertTo-Json -Compress) -ne $definition){throw 'SDR rendition changed stored paint'}
    Assert-Pair 'pair-hdr-rendition'
}

function Swatch-Journey{
    $document=(Model).state.document_file|ConvertTo-Json -Compress
    $ids=@('color-foreground','color-background','color-transparent','color-black','color-white')
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
        $button=Control "color-$slot";$button.SetFocus();[CapyRowPointer]::Key(0x20)
        Wait-Until {(Model).state.colors.slot -eq $slot} "Keyboard did not select $slot"
        if(!$button.Current.HasKeyboardFocus){throw 'Raising the focused swatch lost keyboard focus'}
        Park-Pointer;Assert-Front $slot "keyboard-$slot-front"
    }
    foreach($name in @('black','white')){
        Invoke 'color-background';Wait-Until {(Model).state.colors.slot -eq 'background'} 'Background did not select before quick color'
        Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before quick color'
        Swatch-Tap 'mouse' (Swatch-Point "color-$name")
        Wait-Until {(Model).state.colors.slot -eq 'temporary'} "Quick $name did not select"
        Park-Pointer;Assert-Rim "color-$name" "$name-selected";Assert-Front 'foreground' "$name-front"
        Invoke 'color-transparent';Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent did not select before quick hover'
        $at=Swatch-Point "color-$name";[CapyRowPointer]::Hover($at[0],$at[1]);Assert-Rim "color-$name" "$name-hover"
        Park-Pointer;[CapyRowPointer]::PenHover($at[0],$at[1]);Assert-Rim "color-$name" "$name-pen-hover";[CapyRowPointer]::PenLeave()
    }
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
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'),(@{theme=$theme;language=@{Explicit='en'}}|ConvertTo-Json -Depth 4))
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    # The smoke command strip covers the bottom swatches at this window size.
    # This fixture uses native controls only; keep the isolated profile and all
    # foreground/point ownership checks, without the unrelated overlay.
    Remove-Item Env:CAPY_SMOKE_TEST -ErrorAction SilentlyContinue
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $run 'pid.txt'),[string]$review.Id)
    Write-Output "Compact color review $($review.Id), $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Color fixture did not start' 60
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $panel=(Control 'color-panel').Current.BoundingRectangle
    foreach($id in @('color-background','color-foreground','color-transparent','color-swap','color-shape-0','color-shape-1','color-readout')){
        $bounds=(Control $id).Current.BoundingRectangle
        if($bounds.Left -lt $panel.Left-1 -or $bounds.Top -lt $panel.Top-1 -or $bounds.Right -gt $panel.Right+1 -or $bounds.Bottom -gt $panel.Bottom+1){
            throw "Compact color control is clipped: $id"
        }
    }
    if($Journey -notin @('pair-dark','pair-light')){Swatch-Journey}
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
    $button=Control 'color-shape-0';$target=(Model).color_panel.other_shapes[0];$button.SetFocus();[CapyRowPointer]::Key(0x20)
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
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath (Join-Path $run 'stderr.log')).Length){throw 'Native color stderr requires inspection'}
    $result=[ordered]@{
        theme=$theme;compact_bounds='passed'
        scope='Composed UI pixels and synthetic native input; physical digitizer and hardware performance acceptance remain separate'
    }
    if($Journey -notin @('pair-dark','pair-light')){
        foreach($check in @('swatch_overlap_and_transparent_memory','retained_swatch_focus','circular_swatch_hits','keyboard_activation','document_unchanged')){$result[$check]='passed'}
        if($Journey -ne 'input'){$result['selected_and_hover_rims']='passed'}
    }
    if($Journey -in @('full','pair-dark','pair-light')){$result['retained_header_and_toolbar_pair_pixels']='passed';$result['temporary_active_preview']='passed';$result['mask_alpha_and_rendition']='passed'}
    if($Journey -eq 'full'){
        foreach($check in @('shapes_and_readouts','mouse_pen_touch_fields_and_ring','cancellation','retained_controls','paint_slots_and_swap','native_context_menus','mouse_hold_no_menu','retained_drawer_input')){$result[$check]='passed'}
    }
    $result|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
