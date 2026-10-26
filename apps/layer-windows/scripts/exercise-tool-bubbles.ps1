param([Parameter(Mandatory)][string]$Executable,
    [ValidateSet('dark-mouse','dark-touch','dark-pen','light-mouse','light-touch','light-pen')][string]$Variant='dark-mouse',[switch]$RapidOnly,[switch]$SlidersOnly,[switch]$BlendersOnly,[switch]$HistoryOnly,[switch]$HistoryLightOnly,[switch]$NativeControlsOnly,[switch]$ScrollEpochOnly,[switch]$NumericOnly,[switch]$CurveOnly,[switch]$MenuOnly,[switch]$ObserverOnly,[switch]$VisualOnly,[switch]$BeadTransitionsOnly,[switch]$HdrOnly,[switch]$HdrRetirementOnly,[ValidateSet('F16','F32')][string]$HdrDepth='F16')
$ErrorActionPreference='Stop'
if($HdrRetirementOnly){$HdrOnly=$true}
if($BlendersOnly){$SlidersOnly=$true}
if($NumericOnly -and !$PSBoundParameters.ContainsKey('Variant')){
    foreach($variant in @('dark-touch','light-touch')){& $PSCommandPath -Executable $Executable -Variant $variant -NumericOnly}
    return
}
if($HdrOnly -and !$PSBoundParameters.ContainsKey('Variant')){
    foreach($variant in @('dark-mouse','light-mouse')){& $PSCommandPath -Executable $Executable -Variant $variant -HdrOnly -HdrRetirementOnly:$HdrRetirementOnly -HdrDepth $HdrDepth}
    return
}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'BubbleVisualChecks.ps1')
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
if(!('CapyCanvasTouch' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')}
if(!('CapyBubbleFixtureNative' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'BubbleFixtureNative.cs')}
if(($RapidOnly -or $ObserverOnly -or $BeadTransitionsOnly) -and !('CapyBubbleFoldObserver' -as [type])){
    Add-Type -AssemblyName WindowsBase
    $foldReferences=if($PSVersionTable.PSEdition -eq 'Core'){(Get-ChildItem (Join-Path $PSHOME 'ref/*.dll')).FullName}else{@('System.dll','System.Core.dll','System.Runtime.Serialization.dll','System.Xml.dll')}
    $foldReferences+=@([System.Windows.Automation.AutomationElement].Assembly.Location,[System.Windows.Automation.AutomationIdentifier].Assembly.Location,[System.Windows.Point].Assembly.Location)
    Add-Type -Path (Join-Path $PSScriptRoot 'BubbleFoldObserver.cs') -ReferencedAssemblies ($foldReferences|Select-Object -Unique)
}
if($ObserverOnly){
    $observerFile=Join-Path ([IO.Path]::GetTempPath()) ('CapyFold-'+[Guid]::NewGuid().ToString('N')+'.json');$observer=$null
    try{
        $open='{"ignored":{"state":[1,true,null]},"model":{"bubbles":[{"view":{"id":999,"card":null}},{"view":{"id":123,"card":{"size":[280,305],"full":false}}}]}}'
        [IO.File]::WriteAllText($observerFile,$open);$observer=[CapyBubbleFoldObserver]::new($observerFile,123)
        if(!$observer.OpenSeen -or $observer.FoldSeen){throw 'Typed observer did not distinguish the requested open card'}
        $fold='{"model":{"bubbles":[{"view":{"id":999,"card":{"full":true}}},{"view":{"id":123,"card":null}}]}}'
        [IO.File]::WriteAllText($observerFile,$fold);$watch=[Diagnostics.Stopwatch]::StartNew();while(!$observer.FoldSeen -and $watch.ElapsedMilliseconds -lt 2000){Start-Sleep -Milliseconds 1}
        if(!$observer.FoldSeen -or $observer.FoldSnapshot -ne $fold){throw 'Typed observer did not retain the exact requested fold snapshot'}
        [pscustomobject]@{observer='passed';edition=$PSVersionTable.PSEdition;version=$PSVersionTable.PSVersion.ToString();samples=$observer.Samples}|ConvertTo-Json
    }finally{if($observer){$observer.Dispose()};Remove-Item -LiteralPath $observerFile -ErrorAction SilentlyContinue}
    return
}
$CapyPopups=$true
$Theme,$Device=$Variant.Split('-');if($HistoryLightOnly){$Theme='light';$Variant='light-mouse'};if($NativeControlsOnly -or $ScrollEpochOnly -or $NumericOnly -or $CurveOnly){$Device='touch'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tool-bubbles/'+$Variant+'/'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$checks=[Collections.Generic.List[string]]::new()
$hdrEvidence=[Collections.Generic.List[object]]::new();$bubbleFailed=$false
function Packets {@((Model).bubbles)}
function Packet([long]$Id) {@(Packets|Where-Object {$_.view.id -eq $Id})[0]}
function View([long]$Id) {(Packet $Id).view}
function At([string]$Id,$Within=$root){$b=(Control $Id -Within $Within -Arranged).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Screen([double]$X,[double]$Y){$b=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;@{x=[int][Math]::Round($b.X+$X*$scale);y=[int][Math]::Round($b.Y+$Y*$scale)}}
function Document-Point([double]$X,[double]$Y,[long]$IgnoreBubble=-1){
    if($X -le .01 -or $X -ge .99 -or $Y -le .01 -or $Y -ge .99){throw 'Artwork sample is not safely inside document bounds'}
    $m=Model;$camera=$m.state.camera;$document=@($m.state.tabs|Where-Object active)[0]
    if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Fixture document projection requires an unrotated camera'}
    $point=Screen (($camera.translation[0]+$document.width*$X*$camera.zoom)/$scale) (($camera.translation[1]+$document.height*$Y*$camera.zoom)/$scale)
    foreach($packet in Packets){if($packet.view.id -ne $IgnoreBubble){$center=Center $packet.view.id $packet;$reach=($packet.reach+5)*$scale;if([Math]::Abs($point.x-$center.x) -lt $reach -and [Math]::Abs($point.y-$center.y) -lt $reach){throw 'Artwork sample overlaps another bubble reach'}}}
    return $point
}
function Card-Bounds([long]$Id,$Element=$null,$CardView=$null){
    if(!$Element){$Element=Control "bubble-card-$Id" -Arranged}
    $outer=$Element.Current.BoundingRectangle;$footer=(Control "bubble-properties-$Id" -Within $Element -Arranged).Current.BoundingRectangle
    if(!$CardView){$CardView=(View $Id).card};$height=$CardView.size[1]*$scale
    [pscustomobject]@{X=($footer.Right-$outer.Width);Y=($footer.Bottom-$height);Width=$outer.Width;Height=$height;Bottom=$footer.Bottom;Right=$footer.Right}
}
function Center([long]$Id,$Presentation=$null){
    $card=if(!$Presentation -or $Presentation.view.card){Find "bubble-card-$Id" -Visible}
    if($card){$b=Card-Bounds $Id $card $Presentation.view.card;return @{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
    if(!$Presentation){$Presentation=Packet $Id};$color=$Presentation.color
    if($color){$b=(Control "tool-bubble-$Id" -Arranged).Current.BoundingRectangle;return @{x=[int][Math]::Round($b.X-$color.bounds.x*$scale);y=[int][Math]::Round($b.Y-$color.bounds.y*$scale)}}
    At "tool-bubble-$Id"
}
function Card-Metrics{
    $path=State-File;$state=Read-Snapshot $path
    $name=Split-Path -Leaf $path
    if($name -notmatch '^ui-state-\d+-\d+\.json$'){throw 'Card metrics need the owned drawing trace'}
    $sourcePath=Join-Path (Split-Path -Parent $path) ($name -replace '^ui-state-','panel-metrics-')
    Wait-Until {[IO.File]::Exists($sourcePath)} 'Owned card metrics were not published'
    $source=Read-Snapshot $sourcePath
    if($state.process_id -ne $review.Id -or !$state.model.windows_isolated_settings -or $state.window_id -le 0 -or $source.process_id -ne $review.Id -or $source.window_id -ne $state.window_id){throw 'Card metrics are not from the owned drawing'}
    $metrics=$source.panel_metrics;$values=@{}
    foreach($key in @('control_height','group_spacing','inset')){
        if(!$metrics -or $key -notin $metrics.PSObject.Properties.Name -or $null -eq $metrics.$key){throw 'Shared card metrics are missing'}
        $value=[double]$metrics.$key
        if([double]::IsNaN($value) -or [double]::IsInfinity($value) -or $value -lt 0 -or ($key -eq 'control_height' -and $value -eq 0)){throw 'Shared card metrics are invalid'}
        $values[$key]=$value
    }
    return [pscustomobject]$values
}
function Card-FooterLeft($Bounds,[bool]$Full,[double]$Scale,$Metrics){
    if($Full){return $Bounds.X}
    $padding=6
    $Bounds.X+($Metrics.control_height+2*$padding+$Metrics.group_spacing)*$Scale
}
function Card-RowInside($Row,$Bounds,[double]$Scale,$Metrics){
    !($Row.X -lt $Bounds.X-2 -or $Row.Right -gt $Bounds.Right+2 -or $Row.Y -lt $Bounds.Y+$Metrics.inset*$Scale-2 -or $Row.Bottom -gt $Bounds.Bottom-49*$Scale+2)
}
function Card([long]$Id){
    $metrics=Card-Metrics
    $ready=@{element=$null;expected=$null;actual=$null;footer=$null}
    try{Wait-Until {
        $ready.element=Find "bubble-card-$Id" -Visible;if(!$ready.element -or !(View $Id).card){return $false}
        $bounds=Card-Bounds $Id $ready.element;$size=(View $Id).card.size;$footer=(Control "bubble-properties-$Id" -Within $ready.element -Arranged).Current.BoundingRectangle
        $ready.expected=@(($size[0]*$scale),($size[1]*$scale));$ready.actual=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);$ready.footer=@($footer.X,$footer.Y,$footer.Width,$footer.Height)
        $left=Card-FooterLeft $bounds ([bool](View $Id).card.full) $scale $metrics
        [Math]::Abs($bounds.Width-$size[0]*$scale) -lt 2 -and [Math]::Abs($footer.Height-48*$scale) -lt 2 -and [Math]::Abs($footer.X-$left) -lt 2
    } 'Native card footer did not match published geometry'}catch{@{id=$Id;expected=$ready.expected;inferred_viewport=$ready.actual;native_footer=$ready.footer;panel_metrics=$metrics;expected_footer_x=$left}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'card-geometry-failure.json');throw}
    $bounds=Card-Bounds $Id $ready.element;$rows=@()
    foreach($row in $ready.element.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
        if($row.Current.AutomationId -notmatch '^layer-row-\d+$' -or $row.Current.IsOffscreen){continue}
        $r=$row.Current.BoundingRectangle;$rows+=@{id=$row.Current.AutomationId;bounds=@($r.X,$r.Y,$r.Width,$r.Height)}
        if(!(Card-RowInside $r $bounds $scale $metrics)){
            @{id=$Id;inferred_viewport=$ready.actual;native_footer=$ready.footer;panel_metrics=$metrics;bad_row=@{id=$row.Current.AutomationId;bounds=@($r.X,$r.Y,$r.Width,$r.Height)};rows=$rows}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'card-row-geometry-failure.json')
            throw 'Visible native layer row exceeds the inset card viewport'
        }
    }
    @{id=$Id;inferred_viewport=$ready.actual;native_footer=$ready.footer;rows=$rows;shared_height_used=$true}|ConvertTo-Json -Depth 8 -Compress|Add-Content (Join-Path $run 'card-geometry.jsonl')
    return $ready.element
}
function Inspect-InactiveProperties([long]$Id){
    $card=Card $Id;$properties=Find 'layer-properties' -Within $card
    if(!$properties){throw 'Retained native Properties page is missing'}
    $controls=@($properties.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.IsKeyboardFocusable}|ForEach-Object {@{id=$_.Current.AutomationId;name=$_.Current.Name;offscreen=$_.Current.IsOffscreen;bounds=$_.Current.BoundingRectangle.ToString()}})
    $footer=Control "bubble-properties-$Id" -Within $card;$footer.SetFocus();Wait-Until {$footer.Current.HasKeyboardFocus} 'Inactive-page probe footer did not receive focus'
    $beforeZen=(Model).state.workspace.zen_mode;$focus=@();$inactiveFocus=$false
    foreach($i in 1..12){
        $previous=[System.Windows.Automation.AutomationElement]::FocusedElement
        if($previous.Current.AutomationId -eq 'drawing-canvas'){break}
        $previousId=$previous.GetRuntimeId() -join ':';[CapyRowPointer]::Key([uint32]$review.Id,0x09)
        Wait-Until {([System.Windows.Automation.AutomationElement]::FocusedElement.GetRuntimeId() -join ':') -ne $previousId} 'Inactive-page probe Tab did not move focus'
        $element=[System.Windows.Automation.AutomationElement]::FocusedElement;$focus+=@{id=$element.Current.AutomationId;name=$element.Current.Name}
        for($node=$element;$node;$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)){if(($node.GetRuntimeId() -join ':') -eq ($properties.GetRuntimeId() -join ':')){$inactiveFocus=$true;break};if(($node.GetRuntimeId() -join ':') -eq ($root.GetRuntimeId() -join ':')){break}}
    }
    Wait-Until {(Model).state.workspace.zen_mode -eq $beforeZen} 'Focus probe left a changed Zen state'
    @{controls=$controls;tab_focus=$focus;inactive_page_focused=$inactiveFocus;before_zen=$beforeZen;after_zen=(Model).state.workspace.zen_mode}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'inactive-properties-accessibility.json')
    if($inactiveFocus){throw 'Keyboard Tab reached an inactive Properties page control'}
}
function Relative([long]$Id,$Point){$c=Center $Id;@{x=[int][Math]::Round($c.x+$Point[0]*$scale);y=[int][Math]::Round($c.y+$Point[1]*$scale)}}
function Ring-Point([long]$Id,$Ring,[double]$Fraction){$a=($Ring.arc.start+$Ring.arc.sweep*$Fraction)*[Math]::PI/180;Relative $Id @(($Ring.arc.radius*[Math]::Cos($a)),($Ring.arc.radius*[Math]::Sin($a)))}
function Wheel-FieldPoint([long]$Id){
    $side=(View $Id).diameter-14;$model=(Model).color_panel
    if($model.shape -ne 'circle'){throw 'Wheel fixture requires its registered circle geometry'}
    $point=@(($model.geometry.disc_radius*$side*.5),(-$model.geometry.disc_radius*$side*.25))
    $hit=[CapyBubbleFixtureNative]::ColorHit((Join-Path $directory 'layer_windows.dll'),($point[0]+$side*.5),($point[1]+$side*.5),$side,2)
    if($hit -ne 2){throw 'Published color field point is not a field hit'}
    return $point
}
function Tap($Point){[CapyRowPointer]::Down($Device,$Point.x,$Point.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()}
function Cancel-Contact{if($Device -eq 'mouse'){Escape};[CapyRowPointer]::Cancel()}
function Drag($From,$To,[switch]$Cancel,[string]$PointerDevice=$Device){
    [CapyRowPointer]::Down($PointerDevice,$From.x,$From.y)
    for($i=1;$i -le 16;$i++){[CapyRowPointer]::Move([int]($From.x+($To.x-$From.x)*$i/16),[int]($From.y+($To.y-$From.y)*$i/16));Start-Sleep -Milliseconds 20}
    if($Cancel){Cancel-Contact}else{[CapyRowPointer]::Up()}
}
function Escape{[CapyRowPointer]::Key([uint32]$review.Id,0x1b)}
function Revision{(Model).state.document_file.revision}
function Check-Artwork([string]$Stage,$Expected){
    $m=Model
    @{stage=$Stage;revision=$m.state.document_file.revision;modified=$m.state.document_file.modified;brush=$m.state.brush.preset}|ConvertTo-Json -Compress|Add-Content (Join-Path $run 'revision-checkpoints.jsonl')
    if($m.state.document_file.revision -ne $Expected){throw "$Stage changed artwork revision from $Expected to $($m.state.document_file.revision)"}
}
function Canvas-Baselines{
    $m=Model;$camera=$m.state.camera;$document=@($m.state.tabs|Where-Object active)[0]
    $point=Document-Point .8 .8
    $records=@()
    foreach($case in @(@{name='clear-paper-mouse-tap';device='mouse';stroke=$false},@{name='clear-paper-mouse-stroke';device='mouse';stroke=$true},@{name='clear-paper-pen-stroke';device='pen';stroke=$true})){
        [CapyRowPointer]::Hover($w.X+20,$w.Y+20);$pixels=Wait-StablePixels {Patch-Pixels $point};$revision=Revision
        $started=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        [CapyRowPointer]::Down($case.device,$point.x,$point.y)
        if($case.stroke){for($i=1;$i -le 12;$i++){[CapyRowPointer]::Move($point.x+$i*2,$point.y);Start-Sleep -Milliseconds 15}}else{Start-Sleep -Milliseconds 35}
        [CapyRowPointer]::Up();[CapyRowPointer]::Hover($w.X+20,$w.Y+20);Start-Sleep -Milliseconds 1000
        $after=Revision;$painted=Wait-StablePixels {Patch-Pixels $point}
        $record=@{case=$case.name;screen=$point;started_unix_ms=$started;before_revision=$revision;after_revision=$after;pixels_changed=($pixels -ne $painted)}
        if($after -gt $revision){
            & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Undo
            Wait-Until {(Patch-Pixels $point) -eq $pixels} 'Baseline Undo did not restore exact artwork pixels'
            $record.undo_exact=$true
        }
        $records+=$record;$records|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'canvas-baselines.json')
    }
}
function Paint-Gap([long]$Id,[double[]]$Local=@(33,0),[string]$Tag='gap',[switch]$NoHalo){
    if(!$NoHalo -and !@((View $Id).rings).Count){Tap (Center $Id);Wait-Until {@((View $Id).rings).Count -gt 0} 'Gap check needs an open halo'}
    $view=View $Id
    $request=@{type='hit';view=$view;point=$local}|ConvertTo-Json -Depth 80 -Compress
    $hit=[CapyBubbleFixtureNative]::Query((Join-Path $directory 'layer_windows.dll'),$request)|ConvertFrom-Json
    if($null -ne $hit){throw 'Published halo gap unexpectedly belongs to a bubble'}
    $at=Relative $Id $local;$m=Model;$camera=$m.state.camera;$document=@($m.state.tabs|Where-Object active)[0]
    $surfaceX=($at.x-$w.X)/$scale;$surfaceY=($at.y-$w.Y)/$scale
    $docX=($surfaceX*$scale-$camera.translation[0])/$camera.zoom;$docY=($surfaceY*$scale-$camera.translation[1])/$camera.zoom
    if($docX -lt 5 -or $docY -lt 5 -or $docX -gt $document.width-5 -or $docY -gt $document.height-5){
        throw 'Published gap point is not safely inside the paper projection'
    }
    $checked=Document-Point ($docX/$document.width) ($docY/$document.height) $Id
    if($checked.x -ne $at.x -or $checked.y -ne $at.y){throw 'Gap point does not match document projection'}
    $paintDevice=if($Device -eq 'touch'){'pen'}else{$Device}
    @{local=$local;screen=$at;document=@($docX,$docY);hit=$hit;bubble_device=$Device;canvas_device=$paintDevice}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($Tag+'-input.json'))
    [CapyRowPointer]::Hover($w.X+20,$w.Y+20);$before=Wait-StablePixels {Patch-Pixels $at};$revision=Revision
    [CapyRowPointer]::Down($paintDevice,$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up();[CapyRowPointer]::Hover($w.X+20,$w.Y+20)
    Wait-Until {(Revision) -gt $revision} 'Canvas did not receive contact in published halo gap'
    $painted=Wait-StablePixels {Patch-Pixels $at};if($painted -eq $before){throw 'Gap contact changed revision without painting actual pixels'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Undo
    Wait-Until {(Patch-Pixels $at) -eq $before} 'Undo did not restore exact gap artwork pixels'
    $checks.Add($Tag+'-paints-and-undo-restores-exact-pixels')
}
function Patch-Pixels($Point){
    $bitmap=[Drawing.Bitmap]::new(9,9);$graphics=[Drawing.Graphics]::FromImage($bitmap)
    try{$graphics.CopyFromScreen($Point.x-4,$Point.y-4,0,0,$bitmap.Size);$pixels=foreach($y in 0..8){foreach($x in 0..8){$bitmap.GetPixel($x,$y).ToArgb()}};$pixels -join ','}
    finally{$graphics.Dispose();$bitmap.Dispose()}
}
function Workspace-Key{(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress}
function Settled-Capture([string]$Name){Start-Sleep -Milliseconds 450;Capture $Name -Composed -WithModel}
function Menu-Item([string]$Name){Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
function Hold-Bubble([long]$Id){$p=Center $Id;[CapyRowPointer]::Down($Device,$p.x,$p.y);Wait-Until {$null -ne (Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Hold did not open bubble options';[CapyRowPointer]::Up()}
function Header-Entries {@((Model).header.model.zones|ForEach-Object {$_})}
function Header-Source([string]$Kind,[string]$Value){
    @(Header-Entries|Where-Object {$_.item.kind -eq 'tool' -and $_.item.control.kind -eq $Kind -and (!$Value -or $_.item.control.command -eq $Value -or $_.item.control.panel -eq $Value -or $_.item.control.slot -eq $Value -or [string]$_.item.control.id -eq $Value)}|Where-Object {Find ('header-item-'+$_.id) -Visible})[0]
}
function Ensure-Sources{
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    Invoke 'customize_workspace_ui';Wait-Until {(Model).header.editing} 'Header editor did not open'
    $from=At 'header-component-tools';$p=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json;$z=$p.geometry.zones[1]
    $to=Screen ($z.x+$z.width*.5) ($z.y+$z.height*.5)
    Drag $from $to;Wait-Until {(Model).picker} 'Native tool component did not open picker'
    $wantedSources=@(@('command','drawing_brush'),@('color',''),@('color_picker',''),@('command','undo'),@('panel','layers'),@('command','next_drawing'))
    if($BlendersOnly){$wantedSources=@(@('brush','25'),@('brush','51'),@('brush','55'),@('brush','49'))}
    elseif($SlidersOnly){$wantedSources+=@(@('brush','25'),@('brush','51'),@('brush','55'),@('brush','49'),@('tool_slot','drawing'),@('command','lasso'))}
    foreach($wanted in $wantedSources){
        (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
        Wait-Until {(Model).picker.query -eq ''} 'Picker did not clear search'
        $spec=@((Model).picker.choices|Where-Object {$_.control.kind -eq $wanted[0] -and (!$wanted[1] -or $_.control.command -eq $wanted[1] -or $_.control.panel -eq $wanted[1] -or $_.control.slot -eq $wanted[1] -or [string]$_.control.id -eq $wanted[1])})[0]
        if(!$spec){throw "Shared picker lacks $($wanted -join '/')"}
        $control=$spec.control;$value=if($control.slot){$control.slot}elseif($control.command){$control.command}elseif($control.panel){$control.panel}elseif($control.id){$control.id}elseif($control.tenths){$control.tenths}else{'0'}
        $id='picker-choice-'+$control.kind+'-'+$value
        (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($spec.label)
        Wait-Until {(Model).picker.query -eq $spec.label} 'Picker did not acknowledge source search'
        $choice=Control $id
        $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    }
    Invoke 'Add Tools' -Name -Within (Control 'tool-picker');Wait-Until {!(Model).picker} 'Native picker did not add sources'
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing -and !(Model).windows_workspace.saving} 'Authored fixture header did not settle'
    (Model).state.workspace|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'authored-workspace.json')
}
function Take([string]$Kind,[string]$Value,[double]$X,[double]$Y){
    $entry=Header-Source $Kind $Value;if(!$entry){throw "Missing authored source $Kind/$Value"}
    $before=@(Packets|ForEach-Object {$_.view.id});$from=At ('header-item-'+$entry.id);$to=Screen $X $Y
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    [CapyRowPointer]::Move($from.x,$from.y+[int](80*$scale));[CapyRowPointer]::Move($to.x,$to.y);[CapyRowPointer]::Up()
    Wait-Until {@(Packets|Where-Object {$_.view.id -notin $before}).Count -eq 1} "Source $Kind/$Value did not tear out"
    $id=@(Packets|Where-Object {$_.view.id -notin $before})[0].view.id
    Wait-Until {$null -ne (Find "tool-bubble-$id" -Visible) -or $null -ne (Find "bubble-card-$id" -Visible)} 'Bubble is missing its arranged native view'
    $checks.Add("take-$Kind-$Value")
    $settle=@{last='';same=0}
    Wait-Until {$p=Center $id;$key="$($p.x),$($p.y)";if($key -eq $settle.last){$settle.same++}else{$settle.last=$key;$settle.same=0};$settle.same -ge 5} 'Released bubble did not settle'
    return [long]$id
}
function Slider-Layout{(Model).state.workspace.layout.bubbles|ConvertTo-Json -Depth 70 -Compress}
function Slider-Menu([long]$Id,[bool]$Expected){
    $point=Center $Id;[CapyRowPointer]::RightClick($point.x,$point.y)
    $row=Control 'menu-show-sliders' -Arranged -Type ([System.Windows.Automation.ControlType]::MenuItem)
    $toggle=$row.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if(!$row.Current.IsEnabled -or $row.Current.Name -ne 'Show Sliders' -or ($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On) -ne $Expected){throw 'Native global slider menu differs from the workspace setting'}
    $toggle
}
function Slider-Menus([long[]]$Ids,[bool]$Expected){foreach($id in $Ids){$null=Slider-Menu $id $Expected;Escape;Wait-Until {!(Find 'menu-show-sliders' -Visible)} 'Slider context did not dismiss'}}
function Slider-Toggle([long]$Id,[bool]$Expected){
    $toggle=Slider-Menu $Id (!$Expected);$toggle.Toggle()
    Wait-Until {(Model).state.workspace.layout.bubbles.show_sliders -eq $Expected} 'Global slider preference did not publish'
    $row=Control 'menu-show-sliders' -Arranged -Type ([System.Windows.Automation.ControlType]::MenuItem)
    if(($row.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On) -ne $Expected -or !(Find 'Put back' -Name -Visible)){throw 'Global slider toggle did not keep and refresh its native menu'}
    Escape;Wait-Until {!(Find 'menu-show-sliders' -Visible)} 'Retained slider menu did not dismiss'
}
function Slider-Medium([string]$Group,[double]$X,[double]$Y){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $window=@((Model).application_menus|Where-Object id -eq 'window')[0].model
    $choice=@($window.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.type -eq 'set_panel_visible' -and $_.action.action.panel -eq 'brush_sets'})
    if($choice.Count -ne 1){throw 'The shared Window menu did not offer the brush sets panel'}
    $item=Control $choice[0].label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
    $toggle=$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    if($toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$toggle.Toggle()}else{Escape}
    $tileId='brush-set-'+$Group;$tile=Reveal $tileId
    $ids=@(Packets|ForEach-Object {$_.view.id});$from=At $tileId;Drag $from (Screen $X $Y)
    Wait-Until {@(Packets|Where-Object {$_.view.id -notin $ids}).Count -eq 1} "Native $Group source did not tear out"
    $id=@(Packets|Where-Object {$_.view.id -notin $ids})[0].view.id
    Wait-Until {$null -ne (Find "tool-bubble-$id" -Visible)} "$Group bubble has no arranged native face"
    if((View $id).source.kind -ne 'medium' -or (View $id).source.group -ne $Group){throw "Native $Group tear-out used a different source"}
    Settle-Bubbles;[long]$id
}
function Slider-Hit([long]$Id,[double[]]$Point){
    $request=@{type='hit';view=(View $Id);point=$Point}|ConvertTo-Json -Depth 80 -Compress
    [CapyBubbleFixtureNative]::Query((Join-Path $directory 'layer_windows.dll'),$request)|ConvertFrom-Json
}
function Short-FanOpen([long]$Id){
    if(!(View $Id).selected -or (!@((View $Id).rings).Count -and !@((View $Id).chooser).Count)){Tap (Center $Id);Wait-Until {(View $Id).selected -and @((View $Id).rings).Count -gt 0} 'Medium selection did not open its halo'}
    if(!@((View $Id).chooser).Count){Tap (Center $Id)}
    Wait-Until {@((View $Id).chooser).Count -gt 0} 'Short medium did not open its brush fan'
}
function Short-FanDocumentKey{
    $m=Model
    [ordered]@{file=$m.state.document_file;checkpoint=@($m.windows_tabs.session_stamps|Where-Object {$_.id -eq $m.windows_tabs.selected})[0].stamp.checkpoint}|ConvertTo-Json -Depth 25 -Compress
}
function Invoke-ShortFanChecks{
    $document=Short-FanDocumentKey;$evidence=[Collections.Generic.List[object]]::new()
    foreach($spec in @(@{group='watercolor';presets=@(20,21)},@{group='pencil';presets=@(2,25,26)})){
        $area=(Model).layout.work_area;$id=Slider-Medium $spec.group ($area.x+$area.width*.5) ($area.y+$area.height*.5)
        foreach($pose in @(@{name='center';x=.5;y=.5},@{name='corner';x=.96;y=.96})){
            $area=(Model).layout.work_area;Move-BubbleVisual $id ($area.x+$area.width*$pose.x) ($area.y+$area.height*$pose.y)
            foreach($preset in $spec.presets){
                Short-FanOpen $id;$view=View $id;$items=@($view.chooser)
                if(($items.preview -join ',') -ne ($spec.presets -join ',') -or @($items|Where-Object {$_.kind -ne 'brush' -or !$_.enabled}).Count){throw 'Short fan differs from its exact available preset identities'}
                $null=Assert-BubbleVisualGeometry $id ('short-fan-'+$spec.group+'-'+$pose.name)
                $index=0;while($items[$index].preview -ne $preset){$index++};$item=$items[$index];$hit=Slider-Hit $id $item.center
                if(!$hit -or !($hit.PSObject.Properties.Name -contains 'item') -or $hit.item -ne $index){throw 'Visible short-fan preset point hits a different shared target'}
                $point=Relative $id $item.center;$input=Input-Trace $point.x $point.y
                Tap $point
                Wait-Until {(Model).state.brush.preset -eq $preset -and !@((View $id).chooser).Count -and @((View $id).rings).Count -gt 0} 'Native short-fan preset did not select itself and immediately open its halo'
                if((Short-FanDocumentKey) -cne $document){throw 'Short-fan preset contact changed the document'}
                $evidence.Add(@{group=$spec.group;pose=$pose.name;preset=$preset;index=$index;point=$point;hit=$hit;input=$input;view=$view;selected=(View $id)})
            }
            Short-FanOpen $id;$view=View $id;$face=@(([Math]::Sign($view.chooser[0].center[0])*24.),12.)
            if((Slider-Hit $id $face) -ne 'face'){throw 'Short fan overlaps the material face contact'}
            $preset=(Model).state.brush.preset;Settled-Capture ('short-fan-'+$spec.group+'-'+$pose.name)
            Tap (Relative $id $face)
            Wait-Until {!@((View $id).chooser).Count -and !@((View $id).rings).Count} 'Native material-face contact did not dismiss the fan'
            if((Model).state.brush.preset -ne $preset -or (Short-FanDocumentKey) -cne $document){throw 'Material-face contact selected an overlapping pill or changed the document'}
        }
        $point=Center $id;[CapyRowPointer]::RightClick($point.x,$point.y);(Menu-Item 'Put back').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {$null -eq (Packet $id)} 'Short-fan fixture did not put its medium back'
    }
    [IO.File]::WriteAllText((Join-Path $run 'short-fans.json'),($evidence.ToArray()|ConvertTo-Json -Depth 80),[Text.UTF8Encoding]::new($false))
    $checks.Add('Watercolor-two-and-Pencil-three-native-preset-hit-face-dismissal-and-immediate-halo')
}
function Restart-BubbleReview([string]$LogName='restart-stderr.log'){
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved|Out-Null
    if((Get-Item -LiteralPath $stderr).Length){throw 'Initial slider fixture stderr requires inspection'}
    [CapyRowPointer]::Dispose();$script:stderr=Join-Path $run $LogName
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle;$owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window -and (Model).brush_ready -and (Model).canvas_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Owned slider persistence restart did not become ready' 60
    $script:root=$owned.window.Root;$script:drawingWindow=$owned.window.Handle
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null;[CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
    [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
    Wait-Until {(Model).layout.viewport[0] -gt 1000} 'Restarted slider fixture did not maximize'
}
function Invoke-SliderChecks{
    Invoke-ShortFanChecks
    $area=(Model).layout.work_area
    $medium=Slider-Medium 'pastel' ($area.x+$area.width*.25) ($area.y+$area.height*.7)
    if((Model).state.workspace.layout.bubbles.show_sliders -ne $true){throw 'Fresh workspace slider preference did not default to true'}
    $area=(Model).layout.work_area
    $brush=Take 'brush' '25' ($area.x+$area.width*.25) ($area.y+$area.height*.3)
    $group=Take 'tool_slot' 'drawing' ($area.x+$area.width*.7) ($area.y+$area.height*.3)
    $controls=Take 'color_picker' '' ($area.x+$area.width*.7) ($area.y+$area.height*.7)
    $ids=@($brush,$group,$medium,$controls)
    if((View $brush).source.control.id -ne 25 -or (View $group).source.control.slot -ne 'drawing' -or (View $controls).source.control.kind -ne 'color_picker'){throw 'Native slider fixture did not retain its four distinct sources'}
    Tap (Center $brush);Wait-Until {(View $brush).selected -and @((View $brush).rings).Count -gt 0} 'Individual brush did not select with sliders'
    Slider-Menus $ids $true;Slider-Toggle $group $false;Slider-Menus $ids $false
    foreach($id in $ids){if(@((View $id).rings).Count -or (View $id).stamp){throw 'Global slider off retained a halo or stamp'};$null=Assert-BubbleVisualGeometry $id 'global-sliders-off'}
    if((View $controls).diameter -ne 56){throw 'Hidden controls did not return to ordinary face extent'}
    $added=Take 'command' 'drawing_brush' ($area.x+$area.width*.5) ($area.y+$area.height*.5)
    if(@((View $added).rings).Count){throw 'A new real tear-out ignored the global off preference'}
    $point=Center $added;[CapyRowPointer]::RightClick($point.x,$point.y);(Menu-Item 'Put back').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Packet $added)} 'New slider fixture bubble did not put back'
    foreach($swap in 1..2){$null=Slider-Menu $controls $false;(Menu-Item 'Swap sliders').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();Wait-Until {(View $controls).options.swap -eq ($swap -eq 1)} 'Independent native swap option did not publish';if((Model).state.workspace.layout.bubbles.show_sliders -ne $false){throw 'Per-source swap reset the global slider preference'};Escape}
    $source=Header-Source 'command' 'lasso';Tap (At ('header-item-'+$source.id));Wait-Until {(Model).state.layer_tools.tool -eq 'lasso'} 'Native Lasso header source did not activate'
    Tap (Center $brush);Wait-Until {(View $brush).selected} 'Hidden-slider contact did not select its brush'
    if(@((View $brush).rings).Count -or (View $medium).selected){throw 'Hidden-slider selection reopened sliders or selected the other menu source'}
    $disabled=Slider-Layout;Slider-Toggle $medium $true;Slider-Menus $ids $true
    Wait-Until {@((View $brush).rings).Count -gt 0 -and @((View $controls).rings).Count -eq 2} 'Global enable from another tool did not reopen the selected brush and controls'
    if((View $controls).diameter -ne 134){throw 'Global enable did not restore the controls extent'}
    $null=Assert-BubbleVisualGeometry $brush 'global-sliders-on';$null=Assert-BubbleVisualGeometry $controls 'global-sliders-on';Controls $brush
    $enabled=Slider-Layout
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke 'undo_workspace'
    Wait-Until {(Slider-Layout) -ceq $disabled -and @((View $controls).rings).Count -eq 0} 'Workspace Undo did not restore the exact disabled layout'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke 'redo_workspace'
    Wait-Until {(Slider-Layout) -ceq $enabled -and @((View $controls).rings).Count -eq 2} 'Workspace Redo did not restore the exact enabled layout'
    Slider-Toggle $controls $false
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke 'put_back_bubbles'
    Wait-Until {$b=(Model).state.workspace.layout.bubbles;!@(Packets).Count -and !$b.show_sliders -and !@($b.bubbles).Count -and !@($b.rings).Count -and !@($b.kept).Count -and !(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Empty bubble workspace did not autosave its global off preference'
    $empty=Slider-Layout;Check-Artwork 'global-slider-and-workspace-history' $revision
    Settled-Capture 'global-sliders-empty'
    Restart-BubbleReview
    if((Slider-Layout) -cne $empty){throw 'Native SQLite restart lost the serialized empty-layout global preference'}
    $script:w=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$area=(Model).layout.work_area
    $restored=Take 'command' 'drawing_brush' ($area.x+$area.width*.5) ($area.y+$area.height*.5)
    if((Model).state.workspace.layout.bubbles.show_sliders -ne $false -or @((View $restored).rings).Count){throw 'Restored workspace did not keep new tear-out sliders hidden'}
    $null=Slider-Menu $restored $false;Escape;$null=Assert-BubbleVisualGeometry $restored 'global-sliders-restored'
    Settled-Capture 'global-sliders-restored';Check-Artwork 'restored-slider-preference' $revision
    $checks.Add('global-sliders-four-native-sources-history-empty-SQLite-restart')
    [IO.File]::WriteAllText((Join-Path $run 'global-sliders.json'),(@{disabled=$disabled;enabled=$enabled;empty=$empty;restored=(Slider-Layout);sources=$ids}|ConvertTo-Json -Depth 80),[Text.UTF8Encoding]::new($false))
}

function Assert-BlenderCellSettings($Settings,[string]$CellId,[string[]]$Primary){
    $all=@($Settings);$cell=@($all|Where-Object id -eq $CellId);$spread=@($all|Where-Object id -eq 'spread')
    if($cell.Count -ne 1 -or $spread.Count -ne 1){throw 'Blender did not publish its unique Cell Size and Spread controls'}
    $cell=$cell[0];$spread=$spread[0];$n=$cell.numeric
    if([array]::IndexOf(@($all.id),$cell.id) -ne [array]::IndexOf(@($all.id),$spread.id)+1 -or $cell.group_id -or $spread.group_id -or !$cell.label -or !$spread.label){throw 'Blender setting labels, order or ungrouped primary placement differ from the shared contract'}
    $primarySettings=@($all|Where-Object {!$_.group_id -and $_.id -notin @('size','flow','strength','smoothing_strength')})
    if(($primarySettings.id -join ',') -cne ($Primary -join ',') -or @($primarySettings|Where-Object {!$_.label}).Count){throw 'Blender primary setting order or labels differ from its profile'}
    if($n.kind -ne 'slider' -or $n.mapping.type -ne 'log' -or $n.min -ne 1 -or $n.max -ne 50 -or $n.resolution -ne 1 -or $n.step -ne 1 -or $n.digits -ne 0 -or $n.unit -ne 'px' -or $spread.numeric.mapping.type -ne 'log'){throw 'Blender Cell Size or Spread lost its whole-pixel logarithmic schema'}
    if($Primary -contains 'grit'){
        $grit=@($primarySettings|Where-Object id -eq 'grit')[0].numeric
        if($grit.kind -ne 'slider' -or $grit.mapping.type -ne 'linear' -or $grit.min -ne 0 -or $grit.max -ne 1 -or $grit.unit -ne '%'){throw 'Scatter Grit lost its native percentage slider schema'}
    }
    foreach($value in @($n.log_offset,$spread.numeric.log_offset)){if($null -eq $value -or [double]::IsNaN($value) -or [double]::IsInfinity($value)){throw 'Blender logarithmic metadata is not finite'}}
}
function Blender-Select([int]$Preset){
    $source=Header-Source 'brush' ([string]$Preset);if(!$source){throw 'Blender fixture lost its authored native brush source'}
    Tap (At ('header-item-'+$source.id))
    Wait-Until {(Model).state.brush.preset -eq $Preset -and (Model).brush_ready} 'Native brush source did not prepare its requested preset'
}
function Blender-Field([string]$Id){
    $field=Control ('tool-setting-'+$Id);$field.SetFocus()
    if($field.Current.IsOffscreen){Scroll-Position $field 100}
    Control ('tool-setting-'+$Id) -Arranged
}
function Blender-Value([string]$Id,[string]$Value,[Nullable[double]]$Expected=$null){
    if($null -eq $Expected){$Expected=[double]$Value}
    $canonical=[single]$Expected
    $field=Blender-Field $Id;$field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    [CapyRowPointer]::Key([uint32]$review.Id,13);(Control 'drawing-canvas').SetFocus()
    Wait-Until {[single](@((Model).state.tool_settings|Where-Object id -eq $Id)[0].value) -eq $canonical -and (Model).brush_ready} 'Native Blender numeric field did not commit and prepare its exact value'
}
function Blender-SavedState{
    $m=Model;$stamp=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)
    if($stamp.Count -ne 1 -or $null -eq $stamp[0].stamp.checkpoint){throw 'Blender persistence has no unique current document checkpoint'}
    $c=$m.state.camera;$box=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    [ordered]@{location=$m.state.document_file.location.uri;modified=$m.state.document_file.modified;checkpoint=$stamp[0].stamp.checkpoint;preset=$m.state.brush.preset;
        layers=@($m.state.layers.id);
        settings=@(foreach($setting in $m.state.tool_settings){[ordered]@{id=$setting.id;value=$setting.value}});
        history=@(foreach($command in $m.state.commands){if($command.id -in @('undo','redo')){[ordered]@{id=$command.id;enabled=$command.enabled}}});
        camera=[ordered]@{viewport=$c.viewport;work_area=$c.work_area;translation=$c.translation;zoom=$c.zoom;rotation=$c.rotation;flipped=$c.flipped};native_canvas=@($box.X,$box.Y,$box.Width,$box.Height)}|ConvertTo-Json -Depth 30 -Compress
}
function Invoke-BlenderCellChecks([int]$Preset,[string]$CellId,[double]$DefaultCell,[double]$DefaultSpread,[string[]]$Primary,[string]$Tag){
    $before=Revision;$area=(Model).layout.work_area
    if(@(Packets).Count){& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke 'put_back_bubbles'}
    Wait-Until {!@(Packets).Count -and !(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Blender phase did not retire the completed slider fixture bubbles'
    $id=Take 'brush' ([string]$Preset) ($area.x+$area.width*.25) ($area.y+$area.height*.3)
    if(!(View $id).selected){Tap (Center $id)}
    if((Model).state.workspace.layout.bubbles.show_sliders){Slider-Toggle $id $false}
    Slider-Toggle $id $true
    Wait-Until {(Model).state.brush.preset -eq $Preset -and (Model).brush_ready -and @((View $id).rings).Count -eq $Primary.Count} 'Blender did not prepare its profile primary rings'
    Assert-BlenderCellSettings (Model).state.tool_settings $CellId $Primary
    if(@((Model).state.tool_settings|Where-Object id -eq $CellId)[0].value -ne $DefaultCell -or (@((View $id).rings.setting) -join ',') -cne ($Primary -join ',') -or [Math]::Abs(@((Model).state.tool_settings|Where-Object id -eq 'spread')[0].value-$DefaultSpread) -gt .000001){throw 'Blender default Cell Size or native ring order differs'}
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $menu=@((Model).application_menus|Where-Object id -eq 'window')[0]
    $tool=@($menu.model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.type -eq 'set_panel_visible' -and $_.action.action.panel -eq 'tool_settings'})[0]
    if(!$tool){throw 'Shared Window menu has no Tool settings panel'}
    if(!$tool.selected){Invoke $tool.label -Name}else{Escape}
    if(Find 'panel-tab-tool_settings' -Visible){Invoke 'panel-tab-tool_settings'}
    $cell=Blender-Field $CellId;$cellValue=$cell.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    if([double]$cellValue -ne $DefaultCell){throw 'Native Blender Cell Size does not show its shared default'}
    $observations=@()
    foreach($setting in $Primary){
        $ring=@((View $id).rings|Where-Object setting -eq $setting)[0]
        if($Primary.Count -eq 2 -and (($setting -eq 'spread' -and [Math]::Cos(($ring.arc.start+$ring.arc.sweep*.5)*[Math]::PI/180) -ge 0) -or ($setting -eq $CellId -and [Math]::Cos(($ring.arc.start+$ring.arc.sweep*.5)*[Math]::PI/180) -le 0))){throw 'Blender rings are on the wrong sides of the native face'}
        foreach($fraction in 0.,.5,1.){
            $point=Ring-Point $id $ring $fraction;$hit=Slider-Hit $id @((($point.x-(Center $id).x)/$scale),(($point.y-(Center $id).y)/$scale))
            if($hit -ne 'ring'){throw 'Published Blender ring point does not hit a ring in the actual native runtime'}
            Tap $point
            Wait-Until {
                $field=@((Model).state.tool_settings|Where-Object id -eq $setting)[0];if(!$field -or (View $id).stamp){return $false};$value=$field.value
                if($setting -eq $CellId){return $value -eq [Math]::Floor($value) -and $value -ge 1 -and $value -le 50 -and ($fraction -ne 0 -or $value -eq 1) -and ($fraction -ne 1 -or $value -eq 50) -and ($fraction -ne .5 -or $value -lt 8)}
                [Math]::Abs($value-$(if($fraction -eq 0){0}elseif($fraction -eq 1){1}elseif($setting -eq 'spread'){.09}else{.5})) -lt .011
            } 'Blender native ring did not acknowledge its exact endpoint or small-cell logarithmic midpoint'
            $value=@((Model).state.tool_settings|Where-Object id -eq $setting)[0].value
            $observations+=@{setting=$setting;arc=$ring.arc;fraction=$fraction;point=$point;hit=$hit;value=$value}
        }
    }
    Blender-Value $CellId '12';Blender-Value 'spread' '23 %' .23
    if($Primary -contains 'grit'){Blender-Value 'grit' '65 %' .65}
    $overrides=@((Model).state.tool_settings|Where-Object {$_.id -in $Primary}|Select-Object id,value)|ConvertTo-Json -Compress
    $ring=@((View $id).rings|Where-Object setting -eq $CellId)[0];$from=Ring-Point $id $ring .8;$to=Ring-Point $id $ring .5
    Drag $from $to -Cancel
    Wait-Until {@((Model).state.tool_settings|Where-Object id -eq $CellId)[0].value -eq 12 -and !(View $id).stamp} 'Blender ring cancellation did not restore12'
    $cancelled=@((Model).state.tool_settings|Where-Object {$_.id -in $Primary}|Select-Object id,value)|ConvertTo-Json -Compress
    if($cancelled -cne $overrides){throw 'Cancelled native ring changed another primary override'}
    Blender-Select 51
    if(@((Model).state.tool_settings|Where-Object id -eq $CellId).Count -or @((Model).state.tool_settings|Where-Object id -eq 'spread')[0].numeric.mapping.type -ne 'linear'){throw 'Blender-only Cell Size or logarithmic Spread leaked into Crystal'}
    Blender-Select $Preset
    $retained=@((Model).state.tool_settings|Where-Object {$_.id -in $Primary}|Select-Object id,value)|ConvertTo-Json -Compress
    if($retained -cne $overrides){throw 'Native preset reselection lost Blender primary overrides'}
    if(!@((View $id).rings).Count){Tap (Center $id)}
    $ring=@((View $id).rings|Where-Object setting -eq $CellId)[0];$point=Ring-Point $id $ring .5
    [CapyRowPointer]::DoubleClick($Device,$point.x,$point.y)|Out-Null
    Wait-Until {@((Model).state.tool_settings|Where-Object id -eq $CellId)[0].value -eq $DefaultCell} 'Native Blender Cell Size reset did not restore its shared default'
    Blender-Value 'spread' (([string]($DefaultSpread*100))+' %') $DefaultSpread
    if($Primary -contains 'grit'){Blender-Value 'grit' '100 %' 1}
    $controlState=@((Model).state.tool_settings|Where-Object {$_.id -in $Primary}|Select-Object id,value,numeric,group_id)
    Check-Artwork ($Tag+'-setting-controls') $before
    Blender-Select 25;Blender-Value 'size' '64'
    $clear=Clear-CanvasPoint ('tool-bubble-'+$id) 128 0;$sample=@{x=$clear[0]+64;y=$clear[1]}
    [CapyRowPointer]::Hover($w.X+4,$w.Y+4);$blank=Wait-StablePixels {Screen-Pixels $sample.x $sample.y}
    $blankCheckpoint=Drawing-Checkpoint
    [CapyRowPointer]::Down('pen',$sample.x,($sample.y-16));[CapyRowPointer]::Move($sample.x,($sample.y+16));[CapyRowPointer]::Up()
    Wait-Until {(Revision) -gt $before} 'Blender seed stroke did not commit'
    [CapyRowPointer]::Hover($w.X+4,$w.Y+4);$seed=Wait-StablePixels {Screen-Pixels $sample.x $sample.y}
    $seedCheckpoint=Drawing-Checkpoint
    if($seed -ceq $blank){throw 'Blender fixture seed did not create effective artwork'}
    Blender-Select $Preset;Blender-Value 'size' '64';$seedRevision=Revision
    [CapyRowPointer]::Down('pen',$clear[0],$clear[1]);for($step=1;$step -le 16;$step++){[CapyRowPointer]::Move(($clear[0]+8*$step),$clear[1]);Start-Sleep -Milliseconds 15};[CapyRowPointer]::Up()
    Wait-Until {(Revision) -gt $seedRevision} 'Blender stroke did not commit'
    [CapyRowPointer]::Hover($w.X+4,$w.Y+4);$painted=Wait-StablePixels {Screen-Pixels $sample.x $sample.y}
    $paintedCheckpoint=Drawing-Checkpoint
    if($painted -ceq $seed){throw 'Blender stroke did not change its seeded artwork'}
    Blender-Value $CellId '12'
    $file=Join-Path $run ($Tag+' editing-state.capy');Save-ProjectAs $file
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving -and !(Model).state.document_file.busy} 'Blender editing state did not finish its native autosave'
    $savedState=Blender-SavedState;$savedFile=(Get-FileHash -LiteralPath $file).Hash
    if((Wait-StablePixels {Screen-Pixels $sample.x $sample.y}) -cne $painted){throw 'Saving Blender editing state changed authored pixels'}
    Capture ($Tag+'-session-before-restart') -WithModel -Composed
    Restart-BubbleReview ($Tag+'-restart-stderr.log')
    Wait-Until {(Model).state.document_file.location.uri -eq $file -and !(Model).windows_recovery.busy -and !(Model).windows_recovery.restoring} 'Native Blender session did not reopen its saved drawing'
    $script:w=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    if((Blender-SavedState) -cne $savedState -or @((Model).state.tool_settings|Where-Object id -eq $CellId)[0].value -ne 12){throw 'Native session restart lost Blender12, its source, document checkpoint, history or pixel projection'}
    $nativeCell=(Blender-Field $CellId).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    if((View $id).source.control.id -ne $Preset -or !(View $id).selected -or [double]$nativeCell -ne 12){throw 'Restored Blender source and native Cell Size did not show the saved12'}
    [CapyRowPointer]::Hover($w.X+4,$w.Y+4)
    if((Wait-StablePixels {Screen-Pixels $sample.x $sample.y}) -cne $painted -or (Get-FileHash -LiteralPath $file).Hash -cne $savedFile){throw 'Native Blender session restart changed authored pixels or its saved file'}
    Capture ($Tag+'-session-restored') -WithModel -Composed
    foreach($history in @(@{action='Undo';pixels=$seed;checkpoint=$seedCheckpoint},@{action='Redo';pixels=$painted;checkpoint=$paintedCheckpoint},@{action='Undo';pixels=$seed;checkpoint=$seedCheckpoint},@{action='Undo';pixels=$blank;checkpoint=$blankCheckpoint})){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action $history.action
        Wait-Until {(Screen-Pixels $sample.x $sample.y) -ceq $history.pixels} 'Blender native history did not restore the exact composed pixels'
        if((Drawing-Checkpoint) -cne $history.checkpoint){throw 'Blender Undo or Redo did not restore the exact authored checkpoint and layer order'}
    }
    [IO.File]::WriteAllText((Join-Path $run ($Tag+'-controls.json')),(@{preset=$Preset;primary=$Primary;defaults=@{spread=$DefaultSpread;cell=$DefaultCell};controls=$controlState;overrides=$overrides;cancelled=$cancelled;retained=$retained;schema=(Model).state.tool_settings;rings=$observations;blank=$blank;seed=$seed;painted=$painted;persistence='native saved open drawing and serialized editing-state session restart';saved_state=$savedState;file=$file;file_sha256=$savedFile;physical_input_and_performance='unverified'}|ConvertTo-Json -Depth 30),[Text.UTF8Encoding]::new($false))
    $checks.Add($Tag+'-native-schema-rings-numeric-cancel-reset-retained-overrides-session-persistence-effective-paint-Undo-Redo')
}
function Touch-Pair([int]$AX,[int]$AY,[int]$BX,[int]$BY,[int]$DX,[int]$DY,[double]$Spread,[int]$SecondDelay=16,[string]$Tag='pair'){
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    $targets=@{stage=$Tag;delay_ms=$SecondDelay;first=@($AX,$AY);second=@($BX,$BY);first_window=[CapyBubbleFixtureNative]::WindowAt($AX,$AY)}
    try{
        [CapyCanvasTouch]::Down(1,$AX,$AY);Start-Sleep -Milliseconds $SecondDelay;$targets.second_window=[CapyBubbleFixtureNative]::WindowAt($BX,$BY);[CapyCanvasTouch]::Down(2,$BX,$BY)
        $mx=($AX+$BX)*.5;$my=($AY+$BY)*.5
        foreach($i in 1..24){$t=$i/24.;$factor=1+($Spread-1)*$t;[CapyCanvasTouch]::Pair([int]($mx+($AX-$mx)*$factor+$DX*$t),[int]($my+($AY-$my)*$factor+$DY*$t),[int]($mx+($BX-$mx)*$factor+$DX*$t),[int]($my+($BY-$my)*$factor+$DY*$t));Start-Sleep -Milliseconds 16}
        [CapyCanvasTouch]::Up(2);Start-Sleep -Milliseconds 16;[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose();$targets|ConvertTo-Json -Depth 6 -Compress|Add-Content (Join-Path $run 'pair-native-targets.jsonl')}
}
function Verify-SliderRecovery([long]$Id,[double]$Expected,[string]$Stage){
    $before=Model;$revision=$before.state.document_file.revision;$checkpoint=@($before.windows_tabs.session_stamps|Where-Object id -eq $before.windows_tabs.selected)[0].stamp.checkpoint
    $slider=Control 'layer-opacity-slider' -Within (Card $Id) -Arranged;$b=$slider.Current.BoundingRectangle
    $hover=@{x=[int]($b.X+$b.Width*.85);y=[int]($b.Y+$b.Height*.5)}
    [CapyRowPointer]::Hover($hover.x,$hover.y);Start-Sleep -Milliseconds 350
    $after=Model;$afterCheckpoint=@($after.windows_tabs.session_stamps|Where-Object id -eq $after.windows_tabs.selected)[0].stamp.checkpoint;$opacity=@($after.state.layers|Where-Object editing)[0].opacity
    $evidence=@{expected=$Expected;before_revision=$revision;after_revision=$after.state.document_file.revision;before_checkpoint=$checkpoint;after_checkpoint=$afterCheckpoint;hover_opacity=$opacity}
    $evidence|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ("slider-recovery-$Stage.json"))
    if($opacity -ne $Expected -or $after.state.document_file.revision -ne $revision -or $checkpoint -ne $afterCheckpoint){throw 'Noncontact hover changed native slider value or artwork history after pair cancellation'}
    $settled=@{bounds=$null;card=$null;since=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {$native=(Control 'layer-opacity-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle;$frame=Card-Bounds $Id;$key=@($frame.X,$frame.Y,$frame.Width,$frame.Height)-join ',';if($native -ne $settled.bounds -or $key -ne $settled.card){$settled.bounds=$native;$settled.card=$key;$settled.since.Restart()};$settled.since.ElapsedMilliseconds -ge 250} 'Native slider/card geometry did not settle before recovery drag'
    $b=$settled.bounds;$evidence.drag_before_bounds=@($b.X,$b.Y,$b.Width,$b.Height);$evidence.drag_before_card=$settled.card
    Drag @{x=[int]($b.X+$b.Width*.7);y=[int]($b.Y+$b.Height*.5)} @{x=[int]($b.X+$b.Width*.4);y=[int]($b.Y+$b.Height*.5)}
    $afterBounds=(Control 'layer-opacity-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle;$evidence.drag_after_bounds=@($afterBounds.X,$afterBounds.Y,$afterBounds.Width,$afterBounds.Height)
    try{Wait-Until {$m=Model;$value=@($m.state.layers|Where-Object editing)[0].opacity;$committed=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint;[Math]::Abs($value-.4) -le .05 -and $committed -gt $checkpoint} 'Native fresh slider gesture did not reach40 percent with a new committed checkpoint'}catch{
        $m=Model;$evidence.final_opacity=@($m.state.layers|Where-Object editing)[0].opacity;$evidence.final_checkpoint=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint;$evidence|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ("slider-recovery-$Stage.json"));throw
    }
    $m=Model;$evidence.final_opacity=@($m.state.layers|Where-Object editing)[0].opacity;$evidence.final_checkpoint=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {$m=Model;@($m.state.layers|Where-Object editing)[0].opacity -eq $Expected -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $checkpoint} 'One Undo did not restore the fresh native slider origin and checkpoint'
    $evidence.fresh_gesture_worked=$true;$evidence.undo_restored=$true;$evidence|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ("slider-recovery-$Stage.json"))
}
function Layer-Pair([long]$Id,[int]$Delay=16,[string]$Stage='fast'){
    $card=Card $Id;$control=Control 'layer-opacity-slider' -Within $card -Arranged;$slider=$control.Current.BoundingRectangle
    $opacity=@((Model).state.layers|Where-Object editing)[0].opacity
    $first=@{x=[int]($slider.X+$slider.Width*.3);y=[int]($slider.Y+$slider.Height*.5)};$center=Center $Id;$second=$center
    if($Stage -eq 'near'){$second=@{x=($first.x+[int](30*$scale));y=$first.y}}
    $local=@((($first.x-$center.x)/$scale),(($first.y-$center.y)/$scale))
    $hit=[CapyBubbleFixtureNative]::Query((Join-Path $directory 'layer_windows.dll'),(@{type='hit';view=(View $Id);point=$local}|ConvertTo-Json -Depth 80 -Compress))|ConvertFrom-Json
    if($hit -ne 'card' -and $hit.kind -ne 'card'){throw 'Native slider contact is outside shared card geometry'}
    $contact=@{id=$Id;first=$first;second=$second;first_local=$local;first_hit=$hit;slider_bounds=@($slider.X,$slider.Y,$slider.Width,$slider.Height);opacity=$opacity;delay_ms=$Delay;document_file=(Model).state.document_file;checkpoint=@((Model).windows_tabs.session_stamps|Where-Object id -eq (Model).windows_tabs.selected)[0].stamp.checkpoint}
    $contact|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ("layer-pair-contact-$Stage.json"))
    Touch-Pair $first.x $first.y $second.x $second.y ([int](20*$scale)) ([int](10*$scale)) 1 $Delay $Stage
    Start-Sleep -Milliseconds 400
    $card=Find "bubble-card-$Id";$control=Find 'layer-opacity-slider' -Within $card
    $result=@{expected_opacity=$opacity;actual_opacity=@((Model).state.layers|Where-Object editing)[0].opacity;card_help=$card.Current.HelpText;slider_help=$control.Current.HelpText;canvas_help=(Control 'drawing-canvas').Current.HelpText;canvas_status=(Control 'drawing-canvas').Current.ItemStatus;document_file=(Model).state.document_file;checkpoint=@((Model).windows_tabs.session_stamps|Where-Object id -eq (Model).windows_tabs.selected)[0].stamp.checkpoint}
    $result|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ("layer-pair-observation-$Stage.json"))
    if($result.actual_opacity -eq $result.expected_opacity){try{Verify-SliderRecovery $Id $result.expected_opacity $Stage}catch{$result.recovery_error=$_.Exception.Message;$result|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ("layer-pair-observation-$Stage.json"))}}
    return $result
}
function Settle-LayerScroll($Pattern){
    $stable=@{value=$null;watch=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {$value=$Pattern.Current.VerticalScrollPercent;if($null -eq $stable.value -or [Math]::Abs($value-$stable.value) -gt .001){$stable.value=$value;$stable.watch.Restart()};$stable.watch.ElapsedMilliseconds -ge 250} 'Native list inertia did not settle'
}
function Settled-LayerState{
    $stable=@{key=$null;model=$null;watch=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {$m=Model;$checkpoint=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint;$key=@($m.state.document_file.revision,$m.state.layer_tools.editing_layer.id,$checkpoint,(@($m.state.layers.id)-join ','))-join ':';if($key -ne $stable.key){$stable.key=$key;$stable.watch.Restart()};$stable.model=$m;$stable.watch.ElapsedMilliseconds -ge 250} 'Shared layer publication did not settle after native contact'
    $stable.model
}
function Visible-NativeLayerRows($Card,$List){
    $viewport=$List.Current.BoundingRectangle;$shared=@((Model).state.layers);$rows=@()
    foreach($element in $Card.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
        $idText=$element.Current.AutomationId;if($idText -notmatch '^layer-row-\d+$' -or $element.Current.IsOffscreen){continue};$bounds=$element.Current.BoundingRectangle
        if($bounds.Width -le 0 -or $bounds.Height -le 0 -or $bounds.Y -lt $viewport.Y+2 -or $bounds.Bottom -gt $viewport.Bottom-2){continue}
        $id=[long]($idText -replace '^layer-row-','');$expected=@($shared|Where-Object id -eq $id)
        if($expected.Count -ne 1){throw 'Recycled native row does not map to a current shared layer'}
        $label=Control "layer-$id-name" -Within $element -Arranged
        if($label.Current.Name -ne $expected[0].label){throw 'Recycled native row retains another shared layer label'}
        $rows+=@{id=$id;label=$label.Current.Name;bounds=$bounds;draggable=(@($expected[0].drag_from_handle) -contains 'mouse');runtime_id=@($element.GetRuntimeId())}
    }
    @($rows|Sort-Object {$_.bounds.Y})
}
function Recycle-NativeLayerRows([long]$Id,$Card,$List,$Scroll){
    $records=@();$first=@((Model).state.layers)[0].id;$last=@((Model).state.layers)[-1].id
    foreach($percent in @(100,0,100)){
        Settle-LayerScroll $Scroll;$Scroll.SetScrollPercent(-1,$percent)
        Wait-Until {[Math]::Abs($Scroll.Current.VerticalScrollPercent-$percent) -lt .1} 'Native long-scroll did not reach the requested end'
        Settle-LayerScroll $Scroll;Settle-Bubbles;$visible=@(Visible-NativeLayerRows $Card $List)
        if(!$visible.Count){throw 'Native long-scroll exposes no current shared layer rows'}
        $expected=if($percent -eq 0){$first}else{$last};$edge=Find "layer-row-$expected" -Within $Card -Visible
        if(!$edge){throw 'Native long-scroll did not expose the expected shared end row'}
        $records+=@{percent=$percent;visible=$visible;edge=$expected};$records|ConvertTo-Json -Depth 14|Set-Content (Join-Path $run 'layer-recycle.json')
    }
    $rows=@(Visible-NativeLayerRows $Card $List|Where-Object draggable);if($rows.Count -lt 3){throw 'Recycled viewport lacks reorderable native rows'}
    $source=$rows[-1].id;$target=$rows[1];$before=Model;$order=@($before.state.layers.id)-join ',';$editing=$before.state.layer_tools.editing_layer.id;$checkpoint=@($before.windows_tabs.session_stamps|Where-Object id -eq $before.windows_tabs.selected)[0].stamp.checkpoint
    $grip=Control "layer-$source-drag" -Within $Card -Arranged;$bounds=$grip.Current.BoundingRectangle;[CapyRowPointer]::Hover([int]($bounds.X+$bounds.Width*.5),[int]($bounds.Y+$bounds.Height*.5))
    $grip=Control "layer-$source-drag" -Within $Card -Arranged;$bounds=$grip.Current.BoundingRectangle;$from=@{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*.5)};$to=@{x=[int]($target.bounds.X+$target.bounds.Width*.5);y=[int]($target.bounds.Y+5*$scale)}
    [CapyRowPointer]::Down('mouse',$from.x,$from.y);Start-Sleep -Milliseconds 35
    foreach($step in 1..16){[CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$step/16),[int]($from.y+($to.y-$from.y)*$step/16));Start-Sleep -Milliseconds 20}
    Wait-Until {$gesture=$List.Current.ItemStatus|ConvertFrom-Json;$gesture.phase -eq 'dragging' -and $gesture.can_drop -and $gesture.source -eq $source -and $gesture.target -eq $target.id} 'Recycled row grip did not reach a shared valid drop target'
    [CapyRowPointer]::Up();Wait-Until {(@((Model).state.layers.id)-join ',') -ne $order} 'Recycled row grip drop did not reorder layers'
    $after=Model;$changed=@($after.state.layers.id)-join ','
    if($after.state.layer_tools.editing_layer.id -ne $editing){throw 'Recycled row reorder changed the drawing target'}
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {$m=Model;(@($m.state.layers.id)-join ',') -eq $order -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $checkpoint} 'One Undo did not restore recycled row order and exact checkpoint'
    @{source=$source;target=$target.id;before=$order;after=$changed;undo=(@((Model).state.layers.id)-join ',');checkpoint=$checkpoint}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'layer-recycle-reorder.json')
    $checks.Add('native-long-scroll-recycle-reorder-exact-undo')
}

function Layer-ListContacts([long]$Id){
    $card=Control "bubble-card-$Id" -Arranged
    $editing=(Model).state.layer_tools.editing_layer.id;$row=Control "layer-row-$editing" -Within $card -Arranged;$rowHeight=$row.Current.BoundingRectangle.Height/$scale
    if($rowHeight -le 0){throw 'Native row height is unavailable for overflow setup'}
    $limit=[int][Math]::Ceiling((Model).layout.work_area.height/$rowHeight)+8
    $list=Control 'layer-list' -Within $card -Arranged;$scroll=Scroll-Pattern $list
    for($i=0;$i -lt $limit;$i++){$count=@((Model).state.layers).Count;Invoke 'layer-new' -Within $card;Wait-Until {@((Model).state.layers).Count -eq $count+1} 'Overflow layer was not created';Start-Sleep -Milliseconds 150}
    Settle-Bubbles
    @{row_height=$rowHeight;add_limit=$limit;count=@((Model).state.layers).Count;list_bounds=$list.Current.BoundingRectangle.ToString();shared_card=(View $Id).card}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'list-overflow-setup.json')
    if(!$scroll.Current.VerticallyScrollable){throw 'Native bubble layer list did not overflow'}
    if($ScrollEpochOnly){Native-ScrollEpoch $Id;return}
    Settle-LayerScroll $scroll;$scroll.SetScrollPercent(-1,0);Settle-LayerScroll $scroll
    $beforeInputs=Settled-LayerState;$viewport=$list.Current.BoundingRectangle
    [CapyRowPointer]::Wheel([int]($viewport.X+$viewport.Width*.5),[int]($viewport.Y+$viewport.Height*.5),-120)
    Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'Native mouse wheel did not scroll the layer list'
    Settle-LayerScroll $scroll;$wheelPercent=$scroll.Current.VerticalScrollPercent
    $null=Scroll-Pattern $list -Focus;[CapyRowPointer]::Key([uint32]$review.Id,0x22)
    Wait-Until {$scroll.Current.VerticalScrollPercent -gt $wheelPercent+.5} 'Native Page Down did not scroll the layer list'
    Settle-LayerScroll $scroll;$keyboardPercent=$scroll.Current.VerticalScrollPercent;$afterInputs=Settled-LayerState
    @{wheel_percent=$wheelPercent;page_down_percent=$keyboardPercent;before_revision=$beforeInputs.state.document_file.revision;after_revision=$afterInputs.state.document_file.revision}|ConvertTo-Json|Set-Content (Join-Path $run 'layer-scroll-inputs.json')
    if($afterInputs.state.document_file.revision -ne $beforeInputs.state.document_file.revision -or $afterInputs.state.layer_tools.editing_layer.id -ne $beforeInputs.state.layer_tools.editing_layer.id -or (@($afterInputs.state.layers.id)-join ',') -ne (@($beforeInputs.state.layers.id)-join ',')){throw 'Native wheel or keyboard scrolling changed artwork, selection or layer order'}
    $checks.Add('native-layer-list-wheel-keyboard-and-scroll-automation')
    $records=@();$pairMovementFailure=$false
    foreach($phase in @('single-before','pair-from-scrolling-list','single-after')){
        Settle-LayerScroll $scroll;Settle-Bubbles
        $scroll.SetScrollPercent(-1,0);Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Layer list did not return to top'
        Settle-LayerScroll $scroll;Settle-Bubbles
        $viewport=$list.Current.BoundingRectangle;$rows=@()
        foreach($layer in (Model).state.layers){$label=Find ("layer-$($layer.id)-name") -Within $card;if(!$label){continue};$b=$label.Current.BoundingRectangle;if(!$label.Current.IsOffscreen -and $b.Y -gt $viewport.Y+25 -and $b.Bottom -lt $viewport.Bottom-10){$rows+=@{id=$layer.id;point=@{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}}}}
        if(!$rows.Count){throw 'No interior visible native layer label for touch scrolling'}
        $row=$rows[[int][Math]::Floor($rows.Count/2)];$at=$row.point
        @{phase=$phase;first=$at;row=$row.id;viewport=$viewport.ToString();card=(Card-Bounds $Id);scroll_percent=$scroll.Current.VerticalScrollPercent}|ConvertTo-Json -Depth 10 -Compress|Add-Content (Join-Path $run 'list-contact-admission.jsonl')
        $before=Settled-LayerState;$revision=$before.state.document_file.revision;$order=@($before.state.layers.id)-join ',';$center=Center $Id
        $editing=$before.state.layer_tools.editing_layer.id;$checkpoint=@($before.windows_tabs.session_stamps|Where-Object id -eq $before.windows_tabs.selected)[0].stamp.checkpoint
        $injection=[Collections.Generic.List[object]]::new()
        [CapyCanvasTouch]::Initialize([uint32]$review.Id)
        try{
            $injection.Add(@{phase='down1';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)})
            [CapyCanvasTouch]::Down(1,$at.x,$at.y)
            foreach($dy in @(25,55,90)){Start-Sleep -Milliseconds 35;$endY=[int][Math]::Max($viewport.Y+4,$at.y-$dy);[CapyCanvasTouch]::Move(1,$at.x,$endY)}
            Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'First finger did not actually scroll before pair admission'
            $scrolled=$scroll.Current.VerticalScrollPercent
            if($phase -eq 'pair-from-scrolling-list'){
                $second=@{x=($at.x+[int](35*$scale));y=$endY}
                $injection.Add(@{phase='down2';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)})
                [CapyCanvasTouch]::Down(2,$second.x,$second.y)
                foreach($i in 1..24){[CapyCanvasTouch]::Pair(($at.x+[int](40*$scale*$i/24)),($endY+[int](15*$scale*$i/24)),($second.x+[int](40*$scale*$i/24)),($second.y+[int](15*$scale*$i/24)));Start-Sleep -Milliseconds 16}
                $injection.Add(@{phase='up2-before';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)});[CapyCanvasTouch]::Up(2);$injection.Add(@{phase='up2-accepted';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)});Start-Sleep -Milliseconds 16
            }
            $injection.Add(@{phase='up1-before';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)});[CapyCanvasTouch]::Up(1);$injection.Add(@{phase='up1-accepted';time=([Diagnostics.Stopwatch]::GetTimestamp()*1000.0/[Diagnostics.Stopwatch]::Frequency)})
        }finally{[CapyCanvasTouch]::Dispose();$injection|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ("list-injection-$phase.json"))}
        Settle-LayerScroll $scroll;Settle-Bubbles
        $after=Settled-LayerState;$endCenter=Center $Id
        $afterCheckpoint=@($after.windows_tabs.session_stamps|Where-Object id -eq $after.windows_tabs.selected)[0].stamp.checkpoint
        $record=@{phase=$phase;first=$at;first_scroll_percent=$scrolled;final_scroll_percent=$scroll.Current.VerticalScrollPercent;before_revision=$revision;after_revision=$after.state.document_file.revision;before_editing=$editing;after_editing=$after.state.layer_tools.editing_layer.id;before_checkpoint=$checkpoint;after_checkpoint=$afterCheckpoint;before_center=$center;after_center=$endCenter;order_unchanged=($order -eq (@($after.state.layers.id)-join ','))}
        $records+=$record;$records|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'layer-list-contacts.json')
        if(!$record.order_unchanged -or $record.before_revision -ne $record.after_revision -or $record.before_editing -ne $record.after_editing -or $checkpoint -ne $afterCheckpoint){throw 'Native layer list contact changed artwork, layer order, selection or checkpoint'}
        if($phase -eq 'pair-from-scrolling-list' -and [Math]::Sqrt([Math]::Pow($endCenter.x-$center.x,2)+[Math]::Pow($endCenter.y-$center.y,2)) -lt 10*$scale){$pairMovementFailure=$true}
    }
    $scroll.SetScrollPercent(-1,0);Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Inertia list did not return to top'
    $viewport=$list.Current.BoundingRectangle;$fling=@{x=[int]($viewport.X+$viewport.Width*.5);y=[int]($viewport.Y+$viewport.Height*.7)};$revision=Revision
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{[CapyCanvasTouch]::Down(1,$fling.x,$fling.y);foreach($dy in @(15,40,80)){Start-Sleep -Milliseconds 25;[CapyCanvasTouch]::Move(1,$fling.x,($fling.y-$dy))};[CapyCanvasTouch]::Up(1)}finally{[CapyCanvasTouch]::Dispose()}
    $released=$scroll.Current.VerticalScrollPercent;$tail=@();foreach($delay in @(50,100,200)) {Start-Sleep -Milliseconds $delay;$tail+=@{delay=$delay;percent=$scroll.Current.VerticalScrollPercent}}
    @{at_release=$released;tail=$tail;before_revision=$revision;after_revision=(Revision)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'layer-scroll-inertia.json')
    if(@($tail|Where-Object {$_.percent -gt $released+.1}).Count -eq 0){throw 'Native list fling did not continue scrolling after contact release'}
    if((Revision) -ne $revision){throw 'Native list inertia changed artwork history'}
    Native-InertiaInterruption $Id $list $scroll
    Native-BoundaryReversal $Id $list $scroll
    Settle-LayerScroll $scroll
    $scroll.SetScrollPercent(-1,0);Wait-Until {$scroll.Current.VerticalScrollPercent -lt .1} 'Restored layer list did not return to top'
    $viewport=$list.Current.BoundingRectangle;$selected=$null;$label=$null
    foreach($layer in (Model).state.layers){if($layer.id -eq (Model).state.layer_tools.editing_layer.id){continue};$candidate=Find ("layer-$($layer.id)-name") -Within $card;if(!$candidate){continue};$bounds=$candidate.Current.BoundingRectangle;if(!$candidate.Current.IsOffscreen -and $bounds.Y -gt $viewport.Y+4 -and $bounds.Bottom -lt $viewport.Bottom-4){$selected=$layer.id;$label=$candidate;break}}
    if(!$label){throw 'No visible unselected layer for restored native row selection'}
    $b=$label.Current.BoundingRectangle
    Tap @{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $selected} 'Native row selection did not recover after pair restoration'
    Recycle-NativeLayerRows $Id $card $list $scroll
    if($pairMovementFailure){throw 'Second finger from actively scrolling list did not move the shared bubble; subsequent native list checks completed'}
    $checks.Add('native-list-scroll-pair-and-row-selection-restoration')
}

function Native-ScrollEpoch([long]$Id){
    $original=Settled-LayerState;$originalId=$original.windows_tabs.selected;$file=$original.state.document_file
    $checkpoint=@($original.windows_tabs.session_stamps|Where-Object id -eq $originalId)[0].stamp.checkpoint
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'new_document';Wait-Until {(Model).state.new_drawing} 'Scroll epoch new drawing form did not open'
    New-DrawingPixels 128 96;Invoke 'Create' -Name
    Wait-Until {$m=Model;@($m.windows_tabs.tabs).Count -eq 2 -and $m.brush_ready -and $m.canvas_ready -and $m.state.document_file.epoch -gt $file.epoch -and $m.state.document_file.revision -eq 0} 'Scroll epoch second drawing did not become ready'
    $second=(Model).windows_tabs.selected;$epoch=(Model).state.document_file.epoch;Next-Drawing
    Wait-Until {$m=Model;$m.windows_tabs.selected -eq $originalId -and $m.state.document_file.epoch -gt $epoch -and $m.state.document_file.revision -eq $file.revision -and $m.brush_ready -and $m.canvas_ready} 'Scroll epoch original drawing did not resume'
    if(!(View $Id).card){Tap (At "tool-bubble-$Id")};Wait-Until {$null -ne (View $Id).card} 'Scroll epoch Layers card did not open';Settle-Bubbles
    $card=Card $Id;$list=Control 'layer-list' -Within $card -Arranged;$scroll=Scroll-Pattern $list
    $scroll.SetScrollPercent(-1,0);Settle-LayerScroll $scroll;$box=$list.Current.BoundingRectangle;$at=@{x=[int]($box.X+$box.Width*.5);y=[int]($box.Y+$box.Height*.65)}
    $epoch=(Model).state.document_file.epoch;[CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{[CapyCanvasTouch]::Down(1,$at.x,$at.y);foreach($dy in @(25,55,90)){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$at.x,($at.y-$dy))};Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'Scroll epoch first contact did not actually pan';$beforeSwitch=$scroll.Current.VerticalScrollPercent;Next-Drawing
        Wait-Until {$m=Model;$m.windows_tabs.selected -eq $second -and $m.state.document_file.epoch -gt $epoch -and $m.brush_ready -and $m.canvas_ready} 'Active native scroll did not switch drawing';[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose()}
    $secondState=Settled-LayerState;if($secondState.state.document_file.revision -ne 0 -or $secondState.state.document_file.modified){throw 'Old native scroll mutated the replacement drawing'}
    $epoch=$secondState.state.document_file.epoch;Next-Drawing
    Wait-Until {$m=Model;$m.windows_tabs.selected -eq $originalId -and $m.state.document_file.epoch -gt $epoch -and $m.state.document_file.revision -eq $file.revision -and $m.brush_ready -and $m.canvas_ready} 'Scroll epoch original drawing did not return'
    if(!(View $Id).card){Tap (At "tool-bubble-$Id")};Wait-Until {$null -ne (View $Id).card} 'Scroll epoch returned card did not open';Settle-Bubbles
    $list=Control 'layer-list' -Within (Card $Id) -Arranged;$scroll=Scroll-Pattern $list;Settle-LayerScroll $scroll;$scroll.SetScrollPercent(-1,0);Settle-LayerScroll $scroll
    $box=$list.Current.BoundingRectangle;$at=@{x=[int]($box.X+$box.Width*.5);y=[int]($box.Y+$box.Height*.65)}
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{[CapyCanvasTouch]::Down(1,$at.x,$at.y);foreach($dy in @(25,55,90)){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$at.x,($at.y-$dy))};Wait-Until {$scroll.Current.VerticalScrollPercent -gt .5} 'Fresh native scroll did not resume after drawing switch';$fresh=$scroll.Current.VerticalScrollPercent;[CapyCanvasTouch]::Up(1)}finally{[CapyCanvasTouch]::Dispose()}
    Settle-LayerScroll $scroll;$after=Settled-LayerState;$afterCheckpoint=@($after.windows_tabs.session_stamps|Where-Object id -eq $originalId)[0].stamp.checkpoint
    if($after.state.document_file.revision -ne $file.revision -or $afterCheckpoint -ne $checkpoint){throw 'Drawing-switch scroll changed original artwork history'}
    @{original=$originalId;second=$second;before_switch_scroll=$beforeSwitch;fresh_scroll=$fresh;before_revision=$file.revision;after_revision=$after.state.document_file.revision;checkpoint=$checkpoint;after_checkpoint=$afterCheckpoint}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'native-scroll-document-epoch.json')
    $checks.Add('active-native-scroll-document-switch-and-fresh-scroll')
}
function History-CornerPaint([long]$Id){
    $target=Document-Point .76 .48 $Id;Drag (Center $Id) $target;Settle-Bubbles
    $face=(View $Id).face;$corner=@(($face[0]*.5-1),($face[1]*.5-1))
    Paint-Gap $Id -Local $corner -Tag 'history-corner' -NoHalo
}
function Native-HistoryActions([long]$Id,$Paper,[string]$Before,[string]$Painted){
    $assertButtons={
        param([string]$Stage)
        $attempts=[Collections.Generic.List[object]]::new()
        try{
            Wait-Until {
                $packet=Packet $Id;$specs=@($packet.view.buttons);$aligned=$specs.Count -eq 2
                $attempt=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');shared_count=$specs.Count;buttons=@();aligned=$false}
                try{
                    if($aligned){foreach($i in 0..1){
                        $expected=Relative $Id $packet.history[$i];$side=$packet.view.face[1]*$scale
                        $record=[ordered]@{id="bubble-history-$Id-$i";expected=@{label=$specs[$i].label;enabled=$specs[$i].enabled;center=$expected;width=$side;height=$side};native=$null;aligned=$false}
                        $attempt.buttons+=,$record
                        $button=Find $record.id -Visible
                        if($button){
                            $current=$button.Current;$bounds=$current.BoundingRectangle
                            $record.native=@{label=$current.Name;enabled=$current.IsEnabled;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height)}
                            $record.aligned=$record.native.label -eq $specs[$i].label -and $record.native.enabled -eq $specs[$i].enabled -and !$bounds.IsEmpty -and
                                [Math]::Abs($bounds.X+$bounds.Width*.5-$expected.x) -le 2 -and [Math]::Abs($bounds.Y+$bounds.Height*.5-$expected.y) -le 2 -and
                                [Math]::Abs($bounds.Width-$side) -le 2 -and [Math]::Abs($bounds.Height-$side) -le 2
                        }
                        $aligned=$aligned -and $record.aligned
                    }}
                    $attempt.aligned=$aligned
                    return $aligned
                }finally{$attempts.Add($attempt)}
            } "Native history $Stage label, enabled state or half geometry did not converge with shared projection"
        }finally{@{stage=$Stage;attempts=@($attempts.ToArray())}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run "native-history-actions-$Stage.json")}
    }
    & $assertButtons 'initial'
    Invoke "bubble-history-$Id-0";Wait-Until {(Screen-Pixels $Paper.x $Paper.y) -eq $Before} 'Accessible history Undo did not restore exact pixels';& $assertButtons 'accessibility-undo'
    Invoke "bubble-history-$Id-1";Wait-Until {(Screen-Pixels $Paper.x $Paper.y) -eq $Painted} 'Accessible history Redo did not restore exact pixels';& $assertButtons 'accessibility-redo'
    $undo=Control "bubble-history-$Id-0";Focus-Keyboard $undo -Backward;[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {(Screen-Pixels $Paper.x $Paper.y) -eq $Before} 'Keyboard Enter history Undo did not restore exact pixels';& $assertButtons 'keyboard-undo'
    $redo=Control "bubble-history-$Id-1";Focus-Keyboard $redo -Backward;[CapyRowPointer]::Key([uint32]$review.Id,0x20)
    Wait-Until {(Screen-Pixels $Paper.x $Paper.y) -eq $Painted} 'Keyboard Space history Redo did not restore exact pixels';& $assertButtons 'keyboard-redo'
    $redo=Control "bubble-history-$Id-1";if($redo.Current.IsEnabled -or @((View $Id).buttons)[1].enabled){throw 'Native history Redo was not disabled at history tip'}
    $origin=Settled-LayerState;$disabled=$false
    try{$redo.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}catch [System.Windows.Automation.ElementNotEnabledException]{$disabled=$true}
    if(!$disabled){throw 'Disabled native history action accepted accessibility invocation'}
    $after=Settled-LayerState;if($after.state.document_file.revision -ne $origin.state.document_file.revision -or (Screen-Pixels $Paper.x $Paper.y) -ne $Painted){throw 'Disabled history action changed document or exact pixels'}
    @{theme=$Theme;labels=@((View $Id).buttons.label);enabled=@((View $Id).buttons.enabled);uia='exact Undo/Redo';keyboard='Enter Undo/Space Redo';disabled_invoke_rejected=$disabled}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'native-history-actions.json')
    $checks.Add('native-history-accessibility-keyboard-and-disabled-state')
}
function Native-InertiaInterruption([long]$Id,$List,$Scroll){
    Settle-LayerScroll $Scroll;$Scroll.SetScrollPercent(-1,0);Settle-LayerScroll $Scroll
    $origin=Settled-LayerState;$order=@($origin.state.layers.id)-join ',';$checkpoint=@($origin.windows_tabs.session_stamps|Where-Object id -eq $origin.windows_tabs.selected)[0].stamp.checkpoint
    $box=$List.Current.BoundingRectangle;$point=@{x=[int]($box.X+$box.Width*.5);y=[int]($box.Y+$box.Height*.65)}
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{
        [CapyCanvasTouch]::Down(1,$point.x,$point.y);foreach($dy in @(15,40,80)){Start-Sleep -Milliseconds 25;[CapyCanvasTouch]::Move(1,$point.x,($point.y-$dy))};[CapyCanvasTouch]::Up(1)
        $moving=@{prior=$Scroll.Current.VerticalScrollPercent;before=$null;after=$null}
        Wait-Until {$value=$Scroll.Current.VerticalScrollPercent;if($value -gt $moving.prior+.05){$moving.before=$moving.prior;$moving.after=$value;return $true};$moving.prior=$value;return $false} 'Interrupt fixture did not observe live native inertia'
        [CapyCanvasTouch]::Down(1,$point.x,$point.y)
        foreach($dy in @(25,55,90)){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$point.x,($point.y+$dy))}
        try{Wait-Until {$Scroll.Current.VerticalScrollPercent -lt $moving.after-.5} 'Fresh opposite native pan did not interrupt live inertia'}
        finally{@{inertia_before=$moving.before;inertia_after=$moving.after;opposite_pan=$Scroll.Current.VerticalScrollPercent}|ConvertTo-Json|Set-Content (Join-Path $run 'layer-scroll-interruption-attempt.json')}
        $interrupted=$Scroll.Current.VerticalScrollPercent;[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose()}
    Settle-LayerScroll $Scroll;$Scroll.SetScrollPercent(-1,0);Settle-LayerScroll $Scroll
    $box=$List.Current.BoundingRectangle;$point=@{x=[int]($box.X+$box.Width*.5);y=[int]($box.Y+$box.Height*.65)}
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{[CapyCanvasTouch]::Down(1,$point.x,$point.y);foreach($dy in @(25,55,90)){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$point.x,($point.y-$dy))};Wait-Until {$Scroll.Current.VerticalScrollPercent -gt .5} 'Another fresh native pan failed after inertia interruption';$fresh=$Scroll.Current.VerticalScrollPercent;[CapyCanvasTouch]::Up(1)}finally{[CapyCanvasTouch]::Dispose()}
    Settle-LayerScroll $Scroll;$after=Settled-LayerState;$afterCheckpoint=@($after.windows_tabs.session_stamps|Where-Object id -eq $after.windows_tabs.selected)[0].stamp.checkpoint
    $evidence=@{inertia_before=$moving.before;inertia_after=$moving.after;opposite_pan=$interrupted;fresh_pan=$fresh;before_revision=$origin.state.document_file.revision;after_revision=$after.state.document_file.revision;before_editing=$origin.state.layer_tools.editing_layer.id;after_editing=$after.state.layer_tools.editing_layer.id;before_checkpoint=$checkpoint;after_checkpoint=$afterCheckpoint;order_unchanged=($order -eq (@($after.state.layers.id)-join ','))}
    $evidence|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'layer-scroll-interrupt-inertia.json')
    if(!$evidence.order_unchanged -or $evidence.before_revision -ne $evidence.after_revision -or $evidence.before_editing -ne $evidence.after_editing -or $checkpoint -ne $afterCheckpoint){throw 'Native inertia interruption changed artwork, selection, layer order or checkpoint'}
    $checks.Add('native-live-inertia-interruption-and-fresh-pan')
}
function Native-BoundaryReversal([long]$Id,$List,$Scroll){
    $stateKey={param($model)
        [ordered]@{document=$model.state.document_file;checkpoint=@($model.windows_tabs.session_stamps|Where-Object id -eq $model.windows_tabs.selected)[0].stamp.checkpoint;editing=$model.state.layer_tools.editing_layer;rows=@($model.state.layers|Select-Object id,depth,visible,locked,mask_linked,alpha_locked,pass_through,relationship,selected,editing)}|ConvertTo-Json -Depth 30 -Compress
    }
    Settle-LayerScroll $Scroll;Settle-Bubbles
    $origin=Settled-LayerState;$before=& $stateKey $origin;$center=Center $Id;$records=[Collections.Generic.List[object]]::new()
    try{
        foreach($boundary in @(0,100)){
            $Scroll.SetScrollPercent(-1,$boundary)
            Wait-Until {[Math]::Abs($Scroll.Current.VerticalScrollPercent-$boundary) -lt .1} 'Boundary reversal did not reach its starting edge'
            Settle-LayerScroll $Scroll;$card=Card $Id;$box=$List.Current.BoundingRectangle;$rows=@(Visible-NativeLayerRows $card $List)
            if(!$rows.Count){throw 'Boundary reversal lacks an interior native row'}
            $row=$rows[[int][Math]::Floor($rows.Count/2)];$bounds=(Control "layer-$($row.id)-name" -Within $card -Arranged).Current.BoundingRectangle
            $point=@{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*.5)}
            $distance=[int][Math]::Min(45*$scale,[Math]::Min($point.y-$box.Y,$box.Bottom-$point.y)-8*$scale)
            if($distance -lt 16*$scale){throw 'Boundary reversal lacks native movement clearance'}
            $direction=if($boundary -eq 0){1}else{-1}
            $record=@{boundary=$boundary;from=$point;distance=$distance;viewport=$box.ToString();before_gesture=($List.Current.ItemStatus|ConvertFrom-Json);before_state=$before};$records.Add($record)
            [CapyCanvasTouch]::Initialize([uint32]$review.Id)
            try{
                [CapyCanvasTouch]::Down(1,$point.x,$point.y)
                foreach($step in 1..3){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$point.x,[int]($point.y+$direction*$distance*$step/3))}
                Settle-LayerScroll $Scroll;$record.outward_percent=$Scroll.Current.VerticalScrollPercent;$record.outward_gesture=$List.Current.ItemStatus|ConvertFrom-Json
                if([Math]::Abs($record.outward_percent-$boundary) -ge .1){throw 'Outward native pan escaped its scroll boundary'}
                if($record.outward_gesture.last_cancel.reason -ne 'native_scroll' -or $record.outward_gesture.last_cancel.generation -le $record.before_gesture.generation){throw 'Boundary contact did not enter native vertical pan admission'}
                foreach($step in 1..3){Start-Sleep -Milliseconds 35;[CapyCanvasTouch]::Move(1,$point.x,[int]($point.y+$direction*$distance*(1-2*$step/3)))}
                Wait-Until {if($boundary -eq 0){$Scroll.Current.VerticalScrollPercent -gt .5}else{$Scroll.Current.VerticalScrollPercent -lt 99.5}} 'Held native contact did not reverse away from its scroll boundary'
                $record.reverse_percent=$Scroll.Current.VerticalScrollPercent;$record.reverse_gesture=$List.Current.ItemStatus|ConvertFrom-Json
                [CapyCanvasTouch]::Up(1)
            }finally{[CapyCanvasTouch]::Dispose()}
            Settle-LayerScroll $Scroll;Settle-Bubbles;$after=Settled-LayerState
            $record.after_state=& $stateKey $after;$record.after_gesture=$List.Current.ItemStatus|ConvertFrom-Json;$record.after_center=Center $Id
            if($record.after_state -ne $before -or $record.after_center.x -ne $center.x -or $record.after_center.y -ne $center.y -or $record.after_gesture.phase -ne 'idle' -or $record.after_gesture.captured -or $record.after_gesture.menu_open){throw 'Boundary reversal changed document state, bubble position or retained a row contact'}
        }
    }finally{$records|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'layer-scroll-boundary-reversal.json')}
    $checks.Add('native-held-contact-boundary-reversal-both-ends')
}
function Native-OpacityUndo([double]$Expected,$Checkpoint){
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {$m=Model;[Math]::Abs(@($m.state.layers|Where-Object editing)[0].opacity-$Expected) -lt .001 -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $Checkpoint} 'One native Undo did not restore opacity and exact checkpoint'
}
function Native-NumericContacts([long]$Id){
    $original=Model;$origin=@($original.state.layers|Where-Object editing)[0].opacity;$checkpoint=@($original.windows_tabs.session_stamps|Where-Object id -eq $original.windows_tabs.selected)[0].stamp.checkpoint
    $slider=Control 'layer-opacity-slider' -Within (Card $Id) -Arranged;$range=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
    $range.SetValue(.6);Wait-Until {[Math]::Abs(@((Model).state.layers|Where-Object editing)[0].opacity-.6) -lt .001} 'Native slider RangeValue access did not apply60 percent';Native-OpacityUndo $origin $checkpoint
    $slider.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x25)
    Wait-Until {@((Model).state.layers|Where-Object editing)[0].opacity -lt $origin} 'Stock slider keyboard Arrow Left did not step opacity';Native-OpacityUndo $origin $checkpoint
    $entry=Control 'layer-opacity' -Within (Card $Id) -Arranged;$entry.SetFocus();$value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern);$value.SetValue('50 + 10')
    $readout=$entry.Current.BoundingRectangle
    [CapyRowPointer]::Wheel([int]($readout.X+$readout.Width*.5),[int]($readout.Y+$readout.Height*.5),-120)
    Wait-Until {@((Model).state.layers|Where-Object editing)[0].opacity -lt .6} 'Readout wheel did not commit draft then decrease opacity'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {[Math]::Abs(@((Model).state.layers|Where-Object editing)[0].opacity-.6) -lt .001} 'Readout wheel Undo did not isolate committed draft'
    Native-OpacityUndo $origin $checkpoint
    $checks.Add('native-readout-wheel-commits-draft-one-step-history')
    $entry=Control 'layer-opacity' -Within (Card $Id) -Arranged;$entry.SetFocus();$value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern);$value.SetValue('50 + 10')
    Wait-Until {$value.Current.Value -eq '50 + 10'} 'Native numeric pending expression was not retained'
    $b=(Control 'layer-opacity-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle
    Drag @{x=[int]($b.X+$b.Width*.7);y=[int]($b.Y+$b.Height*.5)} @{x=[int]($b.X+$b.Width*.4);y=[int]($b.Y+$b.Height*.5)}
    Wait-Until {[Math]::Abs(@((Model).state.layers|Where-Object editing)[0].opacity-.4) -lt .05} 'Entry-to-slider drag did not finish40 percent'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {[Math]::Abs(@((Model).state.layers|Where-Object editing)[0].opacity-.6) -lt .001} 'One Undo did not isolate the slider gesture from committed pending expression'
    Native-OpacityUndo $origin $checkpoint
    $entry=Control 'layer-opacity' -Within (Card $Id) -Arranged;$entry.SetFocus();$value=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern);$value.SetValue('1+')
    $revision=Revision;$b=(Control 'layer-opacity-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle
    Drag @{x=[int]($b.X+$b.Width*.7);y=[int]($b.Y+$b.Height*.5)} @{x=[int]($b.X+$b.Width*.4);y=[int]($b.Y+$b.Height*.5)}
    if($value.Current.Value -ne '1+' -or (Revision) -ne $revision -or [Math]::Abs(@((Model).state.layers|Where-Object editing)[0].opacity-$origin) -gt .001){throw 'Slider overwrote refused numeric expression or artwork'}
    $entry.SetFocus();Escape
    @{range_value='passed';keyboard='passed';pending_expression='passed';invalid_draft='passed';original_checkpoint=$checkpoint}|ConvertTo-Json|Set-Content (Join-Path $run 'numeric-contacts.json')
    $checks.Add('native-range-keyboard-entry-to-slider-history')
}
function Curve-Points{ConvertTo-Json -InputObject (@((Model).state.layer_properties.controls|Where-Object key -eq 'rgb')[0].value.value) -Compress -Depth 8}
function Native-PointAdmission($Element,$Point=$null){
    $bounds=$Element.Current.BoundingRectangle;if(!$Point){$Point=@{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*.5)}}
    $hit=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($Point.x,$Point.y));$path=@();$matched=$false
    for($node=$hit;$node;$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)){
        $path+=@{id=$node.Current.AutomationId;name=$node.Current.Name;class=$node.Current.ClassName;bounds=$node.Current.BoundingRectangle.ToString();offscreen=$node.Current.IsOffscreen}
        if(($node.GetRuntimeId()-join ':') -eq ($Element.GetRuntimeId()-join ':')){$matched=$true;break};if(($node.GetRuntimeId()-join ':') -eq ($root.GetRuntimeId()-join ':')){break}
    }
    @{id=$Element.Current.AutomationId;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);point=$Point;native_window=[CapyBubbleFixtureNative]::WindowAt($Point.x,$Point.y);matched=$matched;path=$path}
}

function Native-CurveContacts([long]$Id){
    $ownerLayer=(Model).state.layer_tools.editing_layer.id
    $calibration=@((Native-PointAdmission (Control "bubble-properties-$Id" -Within (Card $Id) -Arranged)),(Native-PointAdmission (Control 'layer-new' -Within (Card $Id) -Arranged)),(Native-PointAdmission (Control 'layer-opacity-slider' -Within (Card $Id) -Arranged)))
    Invoke "bubble-properties-$Id";Wait-Until {(View $Id).card.properties} 'Native Properties page did not open'
    Invoke 'properties-add-filter' -Within (Card $Id)
    $tone=Control 'Tone' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem);$tone.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke 'Curves' -Name;Wait-Until {$null -ne (Find 'property-rgb-curve' -Within (Card $Id))} 'Attached Curves did not appear in native card'
    Settle-Bubbles
    $curve=Control 'property-rgb-curve' -Within (Card $Id) -Arranged;$b=$curve.Current.BoundingRectangle;$point=@{x=[int]($b.X+$b.Width*.5);y=[int]($b.Y+$b.Height*.5)}
    $graphAdmission=Native-PointAdmission $curve $point
    $ancestorBounds=@();for($node=$curve;$node;$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)){$scrollPattern=$null;$ancestorBounds+=@{id=$node.Current.AutomationId;class=$node.Current.ClassName;bounds=$node.Current.BoundingRectangle.ToString();offscreen=$node.Current.IsOffscreen;scroll_clip_candidate=$node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scrollPattern)};if(($node.GetRuntimeId()-join ':') -eq ($root.GetRuntimeId()-join ':')){break}}
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'property-rgb-curve')
    foreach($other in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){if(($other.GetRuntimeId()-join ':') -ne ($curve.GetRuntimeId()-join ':') -and !$other.Current.IsOffscreen){$calibration+=Native-PointAdmission $other}}
    $calibrated=@($calibration|Where-Object matched).Count -gt 0
    @{graph=$graphAdmission;reference_points=$calibration;from_point_calibrated=$calibrated;native_ancestors=$ancestorBounds;native_card=(Card-Bounds $Id);shared_card=(View $Id).card;shared_packet=(Packet $Id);workspace=(Model).layout.work_area}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'curve-contact-admission.json')
    $before=Curve-Points;$m=Model;$checkpoint=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint;$center=Center $Id
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{
        [CapyCanvasTouch]::Down(1,$point.x,$point.y);[CapyCanvasTouch]::Move(1,$point.x,($point.y-[int](30*$scale)))
        Wait-Until {(Curve-Points) -ne $before} 'First native curve finger did not edit curve before takeover'
        $second=Center $Id;[CapyCanvasTouch]::Down(2,$second.x,$second.y)
        foreach($i in 1..24){[CapyCanvasTouch]::Pair(($point.x+[int](25*$scale*$i/24)),($point.y-[int](30*$scale)+[int](10*$scale*$i/24)),($second.x+[int](25*$scale*$i/24)),($second.y+[int](10*$scale*$i/24)));Start-Sleep -Milliseconds 16}
        [CapyCanvasTouch]::Up(2);Start-Sleep -Milliseconds 16;[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose()}
    Wait-Until {$m=Model;(Curve-Points) -eq $before -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $checkpoint} 'Second native curve finger did not roll back exact curve and checkpoint'
    @{before=$before;after=(Curve-Points);checkpoint=$checkpoint;before_center=$center;after_center=(Center $Id)}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'curve-pair-rollback.json')
    Settle-Bubbles
    Invoke "bubble-properties-$Id";Wait-Until {!(View $Id).card.properties} 'All layers page did not open after curve rollback'
    Invoke "layer-$ownerLayer-name" -Within (Card $Id);Wait-Until {(Model).state.layer_properties.layer -eq $ownerLayer} 'Original native owner row did not become active'
    Invoke "bubble-properties-$Id";Wait-Until {(View $Id).card.properties} 'Native owner Properties page did not reopen'
    $prior=Model;$priorOrder=@($prior.state.layers.id)-join ',';$priorCheckpoint=@($prior.windows_tabs.session_stamps|Where-Object id -eq $prior.windows_tabs.selected)[0].stamp.checkpoint
    Invoke 'properties-add-filter' -Within (Card $Id);$tone=Control 'Tone' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem);$tone.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Invoke 'Brightness / Contrast' -Name
    Wait-Until {$null -ne (Find 'property-brightness-slider' -Within (Card $Id)) -and $null -ne (Find 'property-contrast-slider' -Within (Card $Id))} 'Native two-slider filter did not become active'
    Settle-Bubbles
    $values={ $controls=(Model).state.layer_properties.controls;[ordered]@{brightness=@($controls|Where-Object key -eq 'brightness')[0].value.value;contrast=@($controls|Where-Object key -eq 'contrast')[0].value.value}|ConvertTo-Json -Compress }
    $originalValues=& $values;$m=Model;$insertedCheckpoint=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint
    $a=(Control 'property-brightness-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle;$b=(Control 'property-contrast-slider' -Within (Card $Id) -Arranged).Current.BoundingRectangle
    $first=@{x=[int]($a.X+$a.Width*.3);y=[int]($a.Y+$a.Height*.5)};$second=@{x=[int]($b.X+$b.Width*.7);y=[int]($b.Y+$b.Height*.5)}
    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    try{
        [CapyCanvasTouch]::Down(1,$first.x,$first.y)
        Wait-Until {(& $values) -ne $originalValues} 'First actual slider did not preview its numeric edit'
        [CapyCanvasTouch]::Down(2,$second.x,$second.y)
        foreach($i in 1..24){[CapyCanvasTouch]::Pair(($first.x+[int](20*$scale*$i/24)),($first.y+[int](10*$scale*$i/24)),($second.x+[int](20*$scale*$i/24)),($second.y+[int](10*$scale*$i/24)));Start-Sleep -Milliseconds 16}
        [CapyCanvasTouch]::Up(2);Start-Sleep -Milliseconds 16;[CapyCanvasTouch]::Up(1)
    }finally{[CapyCanvasTouch]::Dispose()}
    Wait-Until {$m=Model;(& $values) -eq $originalValues -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $insertedCheckpoint} 'Two different native sliders failed exact value/history rollback'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x5A)
    Wait-Until {$m=Model;(@($m.state.layers.id)-join ',') -eq $priorOrder -and @($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp.checkpoint -eq $priorCheckpoint} 'One Undo after numeric handoff did not reach the prior layer order/checkpoint'
    @{first=$first;second=$second;values=$originalValues;pair_checkpoint=$insertedCheckpoint;undo_checkpoint=$priorCheckpoint;order_after_undo=(@((Model).state.layers.id)-join ',')}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'different-numeric-pair.json')
    $checks.Add('different-native-numeric-controls-pair-history')
    $checks.Add('native-curve-pair-exact-rollback')
}

function Controls([long]$Id){
    $ring=@((View $Id).rings|Where-Object enabled)[0]
    if(!$ring){throw 'The default profile has no enabled halo'}
    $setting=$ring.setting;$value={@((Model).state.tool_settings|Where-Object id -eq $setting)[0].value}
    $start=& $value;$from=Ring-Point $Id $ring .2;$to=Ring-Point $Id $ring .8
    [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {$null -ne (View $Id).stamp -and (& $value) -ne $start} "Held $setting arc did not update value/dab"
    $stamp=(View $Id).stamp;$sample=Relative $Id @(($stamp.center[0]+$stamp.diameter*.3),$stamp.center[1])
    $held=Wait-StablePixels {Patch-Pixels $sample}
    Capture 'held-profile-dab' -Composed -WithModel
    Cancel-Contact;Wait-Until {$null -eq (View $Id).stamp -and [Math]::Abs((& $value)-$start) -lt .001} 'Arc cancellation did not restore its value and retire dab'
    Wait-Until {(Patch-Pixels $sample) -ne $held} 'Dab model changed but actual preview pixels were blank'
    Drag $from $to;Wait-Until {(& $value) -ne $start} "$setting arc did not commit"
    [CapyRowPointer]::DoubleClick($Device,$to.x,$to.y)|Out-Null
    Wait-Until {[Math]::Abs((& $value)-$start) -lt .001} "Double contact did not restore the $setting default"
    if($Device -eq 'touch'){
        $first=Ring-Point $Id $ring .8;$second=Center $Id
        Touch-Pair $first.x $first.y $second.x $second.y ([int](25*$scale)) ([int](15*$scale)) 1
        Wait-Until {$null -eq (View $Id).stamp -and [Math]::Abs((& $value)-$start) -lt .001} 'Second finger did not cancel the first arc edit/dab'
        $checks.Add('pair-cancels-native-arc-contact')
    }
    $checks.Add('arc-dab-cancel-reset')
}
function Invoke-RetiredMenu($Pattern,[string]$Evidence){
    $errors=@();$retired=$false
    try{$Pattern.Invoke()}catch{
        for($nativeError=$_.Exception;$nativeError;$nativeError=$nativeError.InnerException){$errors+=@{type=$nativeError.GetType().FullName;hresult=$nativeError.HResult;message=$nativeError.Message};if($nativeError -is [System.Runtime.InteropServices.COMException] -or $nativeError -is [System.Windows.Automation.ElementNotAvailableException] -or $nativeError -is [System.InvalidOperationException]){$retired=$true}}
        if(!$retired){throw}
    }
    @{peer_retired=$retired;errors=$errors}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run $Evidence)
    return $retired
}
function Next-Drawing{
    $source=@{entry=$null}
    Wait-Until {$source.entry=Header-Source 'command' 'next_drawing';$null -ne $source.entry} 'Next drawing header source did not become visible'
    $frame=Control ('header-item-'+$source.entry.id)
    foreach($button in $frame.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))){
        if($button.Current.IsOffscreen -or !$button.Current.IsEnabled){continue};$pattern=$null
        if($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke();return}
    }
    throw 'Next drawing header source lacks its native action button'
}
function Settle-Bubbles{
    $settle=@{last='';same=0}
    Wait-Until {$key=@(Packets|ForEach-Object {$point=Center $_.view.id $_;"$($_.view.id):$($point.x),$($point.y)"}) -join '|';if($key -eq $settle.last){$settle.same++}else{$settle.last=$key;$settle.same=0};$settle.same -ge 4} 'Native bubbles did not settle after document activation'
}
function Document-SwitchMenu([long]$Id){
    $original=(Model).windows_tabs.selected;$originalFile=(Model).state.document_file
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke 'new_document';Wait-Until {(Model).state.new_drawing} 'New drawing form did not open'
    New-DrawingPixels 128 96;Invoke 'Create' -Name
    Wait-Until {$m=Model;@($m.windows_tabs.tabs).Count -eq 2 -and $m.brush_ready -and $m.canvas_ready -and $m.state.document_file.epoch -gt $originalFile.epoch -and !$m.state.document_file.modified -and $m.state.document_file.revision -eq 0} 'Second drawing did not become ready'
    $second=(Model).windows_tabs.selected;$secondEpoch=(Model).state.document_file.epoch
    Next-Drawing;Wait-Until {$m=Model;$m.windows_tabs.selected -eq $original -and $m.state.document_file.epoch -gt $secondEpoch -and $m.state.document_file.revision -eq $originalFile.revision -and $m.state.document_file.modified -eq $originalFile.modified -and $m.brush_ready -and $m.canvas_ready} 'Original drawing did not resume'
    Settle-Bubbles
    $layout=Workspace-Key;$count=@(Packets).Count
    $point=Center $Id;@{id=$Id;point=$point;selected=(Model).windows_tabs.selected;document_file=(Model).state.document_file;original_file=$originalFile;brush_ready=(Model).brush_ready;canvas_ready=(Model).canvas_ready;phase='before-old-menu'}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'document-menu-point.json');[CapyRowPointer]::RightClick($point.x,$point.y)
    $stale=Menu-Item 'Put back';$action=$stale.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $priorEpoch=(Model).state.document_file.epoch;Next-Drawing
    Wait-Until {$m=Model;$m.windows_tabs.selected -eq $second -and $m.state.document_file.epoch -gt $priorEpoch -and $m.state.document_file.revision -eq 0 -and $m.brush_ready -and $m.canvas_ready -and !(Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Document switch retained the old native bubble menu'
    $retired=Invoke-RetiredMenu $action 'document-menu-uia-errors.json'
    Start-Sleep -Milliseconds 250
    if(@(Packets).Count -ne $count -or (Workspace-Key) -ne $layout -or (Model).state.document_file.modified){throw 'Retired document menu action changed the new drawing or bubble layout'}
    Settle-Bubbles
    $point=Center $Id;[CapyRowPointer]::RightClick($point.x,$point.y);$null=Menu-Item 'Put back';Escape
    $priorEpoch=(Model).state.document_file.epoch;Next-Drawing;Wait-Until {$m=Model;$m.windows_tabs.selected -eq $original -and $m.state.document_file.epoch -gt $priorEpoch -and $m.state.document_file.revision -eq $originalFile.revision -and $m.brush_ready -and $m.canvas_ready} 'Original drawing did not restore after menu regression'
    $checks.Add('document-switch-retires-secondary-menu-and-stale-action')
}
function Restore-Bubble([long]$Id){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke 'undo_workspace'
    Wait-Until {$null -ne (Packet $Id)} 'Workspace Undo did not restore the same bubble id'
    $null=Control "tool-bubble-$Id" -Arranged
}
function Retained-Menu([long]$Id){
    $point=Center $Id;[CapyRowPointer]::RightClick($point.x,$point.y)
    $old=(Menu-Item 'Put back').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern);$old.Invoke()
    Wait-Until {$null -eq (Packet $Id)} 'Native Put back did not remove bubble'
    Restore-Bubble $Id
    $point=Center $Id;[CapyRowPointer]::RightClick($point.x,$point.y);$fresh=Menu-Item 'Put back';$before=Workspace-Key;$retired=$false
    $retired=Invoke-RetiredMenu $old 'retired-menu-uia-errors.json'
    Start-Sleep -Milliseconds 300
    if(!$retired -and (!(Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)))){throw 'Old menu action dismissed the fresh owner'}
    if($null -eq (Packet $Id) -or (Workspace-Key) -ne $before){throw 'Retired menu action changed the restored same-id bubble'}
    (Menu-Item 'Put back').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Packet $Id)} 'Fresh restored-bubble menu did not work'
    Restore-Bubble $Id
    @{id=$Id;old_peer_retired=$retired;layout_preserved=$true;fresh_menu_worked=$true}|ConvertTo-Json|Set-Content (Join-Path $run 'same-id-menu-retirement.json')
    $checks.Add('same-document-restored-id-rejects-old-menu')
}
function Rapid-Reopen([long]$Id){
    if($RapidOnly){
        foreach($packet in @(Packets|Where-Object {$_.view.id -ne $Id})){$point=Center $packet.view.id $packet;[CapyRowPointer]::RightClick($point.x,$point.y);(Menu-Item 'Put back').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();Wait-Until {$null -eq (Packet $packet.view.id)} 'Rapid fixture could not clear neighbor'}
        $bounds=Card-Bounds $Id;$current=Center $Id;$area=(Model).layout.work_area;$desired=Screen ($area.x+$area.width*.5) ($area.y+$area.height*.5)
        $from=@{x=[int]($bounds.X+3*$scale);y=[int]($bounds.Y+$bounds.Height*.5)};$to=@{x=($from.x+$desired.x-$current.x);y=($from.y+$desired.y-$current.y)}
        Drag $from $to;Start-Sleep -Milliseconds 500
        Wait-Until {
            $m=Model;$item=@($m.state.workspace.layout.bubbles.bubbles|Where-Object id -eq $Id)[0];$basis=@(0,0)
            if($item.corner -match 'right'){$basis[0]=$m.layout.viewport[0]};if($item.corner -match 'bottom'){$basis[1]=$m.layout.viewport[1]}
            $authored=Screen ($basis[0]+$item.offset[0]) ($basis[1]+$item.offset[1]);$visible=Center $Id
            [Math]::Abs($authored.x-$visible.x) -lt 2 -and [Math]::Abs($authored.y-$visible.y) -lt 2
        } 'Rapid card visible center does not match its authored position'
    }
    $center=Center $Id;$outside=Screen ($width*.90) ($height*.78)
    if($RapidOnly){
        $observer=[CapyBubbleFoldObserver]::new((State-File),$Id,(Card $Id))
        try{$contactMilliseconds=[CapyRowPointer]::RapidReopen($Device,$outside.x,$outside.y,$center.x,$center.y,$observer.NativeFoldSignal);$reopenMilliseconds=([CapyRowPointer]::RapidReopenDownTicks-$observer.NativeFoldDetectedTicks)*1000.0/[Diagnostics.Stopwatch]::Frequency;Start-Sleep -Milliseconds 100}
        finally{$observer.Dispose()}
        $foldedFace=Find "tool-bubble-$Id"
        $nativeIntervalUpper=([CapyRowPointer]::RapidReopenDownTicks-$observer.NativeLastOpenTicks)*1000.0/[Diagnostics.Stopwatch]::Frequency
        @{fold_to_reopen_down_ms=$reopenMilliseconds;outside_to_reopen_ms=$contactMilliseconds;fold_duration_ms=150;observer='native-card-peer';model_observation='unobserved';fold_observed=($observer.NativeFoldDetectedTicks -gt 0);native_fold_observed=($observer.NativeFoldDetectedTicks -gt 0);native_fold_ticks=$observer.NativeFoldDetectedTicks;native_last_open_ticks=$observer.NativeLastOpenTicks;native_fold_to_down_upper_bound_ms=$nativeIntervalUpper;reopen_down_ticks=[CapyRowPointer]::RapidReopenDownTicks;samples=$observer.Samples;reopen_point=$center;outside_point=$outside;native_face_bounds=$(if($foldedFace){$foldedFace.Current.BoundingRectangle.ToString()}else{$null})}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'rapid-reopen-timing.json')
        if($observer.FoldSnapshot){[IO.File]::WriteAllText((Join-Path $run 'rapid-fold-snapshot.json'),$observer.FoldSnapshot)}
        if($reopenMilliseconds -lt 0 -or $reopenMilliseconds -ge 150 -or $observer.NativeFoldDetectedTicks -eq 0 -or $nativeIntervalUpper -ge 150){throw "Rapid reopen not exercised: native interval upper bound $nativeIntervalUpper ms; detection to Down $reopenMilliseconds ms (limit 150 ms)"}
    }else{
        Tap $outside;Wait-Until {$null -eq (View $Id).card} 'Outside contact did not fold card'
        $face=Control "tool-bubble-$Id" -Arranged;Tap (At "tool-bubble-$Id")
    }
    Wait-Until {$null -ne (View $Id).card} 'Native immediate tap did not reopen folding card'
    $card=Card $Id;$count=@((Model).state.layers).Count
    Invoke 'layer-new' -Within $card
    Wait-Until {@((Model).state.layers).Count -eq $count+1} 'Rapidly reopened card retained an inert snapshot instead of native footer'
    Invoke "bubble-properties-$Id";Wait-Until {(View $Id).card.properties} 'Rapidly reopened native Properties is inert'
    Invoke "bubble-properties-$Id";Wait-Until {!(View $Id).card.properties} 'Rapidly reopened Properties did not return to Layers'
    $checks.Add($(if($RapidOnly){'rapid-fold-reopen-live-inspector'}else{'fold-reopen-live-inspector'}))
}
try{
    Enter-CapyEnvironment @('CAPY_LATENCY_TRACE')
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_LATENCY_TRACE='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $settings=@{theme=$Theme};if($SlidersOnly){$settings.language=@{Explicit='en'}}
    [IO.File]::WriteAllText((Settings-File),($settings|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -PassThru -RedirectStandardError $stderr
    $null=$review.Handle;$owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Owned bubble fixture did not become ready' 60
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));$null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id);$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
    Wait-Until {(Model).layout.viewport[0] -gt 1000} 'Owned fixture did not maximize'
    if((Model).windows_workspace.id -ne 'builtin:workspace:painter'){
        $choice=Find 'workspace-switch-builtin:workspace:painter' -Visible
        if(!$choice){Invoke 'header-workspace-menu';$choice=Control 'Sketch' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:painter' -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Fixture Sketch workspace did not open'
    }
    if($HdrOnly){New-BubbleHdrDrawing $HdrDepth}
    Ensure-Sources
    if($HdrOnly){Show-BubbleHdrColor}
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'
    Invoke 'fit_canvas'
    Wait-Until {
        $m=Model;$camera=$m.state.camera;$document=@($m.state.tabs|Where-Object active)[0];$area=$camera.work_area
        $extent=@(($document.width*$camera.zoom),($document.height*$camera.zoom))
        !(Find 'fit_canvas' -Visible) -and $m.canvas_ready -and
            [Math]::Max($extent[0]/$area[2],$extent[1]/$area[3]) -gt .5 -and
            $camera.translation[0] -ge $area[0] -and $camera.translation[1] -ge $area[1] -and
            $camera.translation[0]+$extent[0] -le $area[0]+$area[2]+1 -and $camera.translation[1]+$extent[1] -le $area[1]+$area[3]+1
    } 'Fixture drawing did not fit the maximized work area'
    $w=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$width=(Model).layout.viewport[0];$height=(Model).layout.viewport[1]
    $revision=Revision
    $initial=Model;$camera=$initial.state.camera;$document=@($initial.state.tabs|Where-Object active)[0]
    if($HdrOnly){
        Invoke-BubbleHdrChecks
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved|Out-Null
        if((Get-Item -LiteralPath $stderr).Length){throw 'HDR bubble native stderr requires inspection'}
        $scope=if($HdrRetirementOnly){'HDR color context retirement, clear native status and painting with exact Undo after drawing activation'}else{'HDR color bubble native input, accessibility, geometry and composed pixels'}
        @{variant=$Variant;depth=$HdrDepth;checks=@($checks);scope=($scope+'; physical digitizers and HDR display luminance unverified')}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'results.json')
        return
    }
    if($SlidersOnly){
        if(!$BlendersOnly){Invoke-SliderChecks}
        Invoke-BlenderCellChecks 55 'dither_cell_size' 4 .08 @('spread','dither_cell_size') 'dither55'
        Invoke-BlenderCellChecks 49 'scatter_cell_size' 3 .2 @('spread','scatter_cell_size','grit') 'scatter49'
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved|Out-Null
        if((Get-Item -LiteralPath $stderr).Length){throw 'Restarted slider fixture stderr requires inspection'}
        $scope=if($BlendersOnly){'Native Blender panel controls, rings, painting and editing-state persistence'}else{'Native global slider menus, history and SQLite persistence'}
        [IO.File]::WriteAllText((Join-Path $run 'results.json'),(@{variant=$Variant;blenders_only=[bool]$BlendersOnly;checks=@($checks);scope=($scope+'; physical input and performance separate')}|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
        return
    }
    if(!$NumericOnly -and !$CurveOnly){
    $dial=Take 'command' 'drawing_brush' (($camera.translation[0]+$document.width*.35*$camera.zoom)/$scale) (($camera.translation[1]+$document.height*.35*$camera.zoom)/$scale)
    Check-Artwork 'tear-out' $revision
    if($MenuOnly){Document-SwitchMenu $dial;@{checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved|Out-Null;return}
    Wait-Until {@((View $dial).rings).Count -gt 0} 'Selected brush did not tear out with halo'
    Assert-BubbleVisualStates $dial
    Tap (Center $dial);Wait-Until {@((View $dial).chooser).Count -gt 0} 'Dial did not open chooser'
    Check-Artwork 'open-chooser' $revision
    Settled-Capture 'chooser'
    Assert-BubbleVisualChooserTags $dial
    $medium=@((View $dial).chooser|Where-Object kind -ne 'brush')[0]
    Tap (Relative $dial $medium.center);Wait-Until {@((View $dial).chooser|Where-Object kind -eq 'brush').Count -gt 0} 'Medium did not fan brushes'
    Check-Artwork 'choose-medium' $revision
    $brush=@((View $dial).chooser|Where-Object kind -eq 'brush')[0]
    Tap (Relative $dial $brush.center);Wait-Until {@((View $dial).chooser).Count -eq 0} 'Brush choice did not close chooser'
    Wait-Until {@((View $dial).rings).Count -gt 0} 'Chooser brush choice did not immediately open sliders'
    Controls $dial
    Check-Artwork 'choose-brush' $revision
    $checks.Add('dial-medium-brush-chooser')
    Hold-Bubble $dial;Settled-Capture 'hold-menu';Escape
    Check-Artwork 'hold-menu-and-escape' $revision
    Wait-Until {!(Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Escape did not dismiss bubble menu'
    if((Revision) -ne $revision){throw 'Bubble configuration changed artwork revision'}
    Canvas-Baselines
    Paint-Gap $dial
    $before=Workspace-Key;$position=Center $dial
    Drag $position @{x=$position.x+[int](70*$scale);y=$position.y+[int](20*$scale)} -Cancel
    Wait-Until {(Workspace-Key) -eq $before} 'Cancelled bubble drag changed its saved placement'
    $checks.Add('drag-cancel-restores-workspace')
    $controls=Take 'color_picker' '' ($width*.30) ($height*.32)
    Controls $controls
    $history=Take 'command' 'undo' ($width*.52) ($height*.75)
    $paper=Document-Point .72 .85;$m=Model;$document=@($m.state.tabs|Where-Object active)[0];$end=Document-Point (.72+60*$scale/$m.state.camera.zoom/$document.width) .85
    [CapyRowPointer]::Hover($w.X+20,$w.Y+20)
    $before=Wait-StablePixels {Screen-Pixels $paper.x $paper.y}
    $paintDevice=if($Device -eq 'touch'){'pen'}else{$Device}
    Drag $paper $end -PointerDevice $paintDevice
    [CapyRowPointer]::Hover($w.X+20,$w.Y+20)
    $painted=Wait-StablePixels {Screen-Pixels $paper.x $paper.y}
    if($painted -eq $before){throw 'Native canvas stroke did not change actual pixels'}
    if(@((View $controls).rings).Count -eq 0){throw 'Painting closed the controls halo'}
    Tap (Relative $history (Packet $history).history[0]);Wait-Until {(Screen-Pixels $paper.x $paper.y) -eq $before} 'Bubble Undo did not restore exact artwork pixels'
    Tap (Relative $history (Packet $history).history[1]);Wait-Until {(Screen-Pixels $paper.x $paper.y) -eq $painted} 'Bubble Redo did not restore exact stroke pixels'
    $checks.Add('history-exact-artwork')
    if($HistoryOnly -or $HistoryLightOnly){Native-HistoryActions $history $paper $before $painted;History-CornerPaint $history;@{checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved|Out-Null;return}
    }
    $layers=Take 'panel' 'layers' ($width*.38) ($height*.56)
    Wait-Until {$null -ne (View $layers).card} 'Layers did not tear out open'
    $card=Card $layers;$count=@((Model).state.layers).Count
    $add=Find "bubble-new-layer-$layers" -Within $card
    if($add){Invoke "bubble-new-layer-$layers" -Within $card}else{Invoke 'layer-new' -Within $card}
    Wait-Until {@((Model).state.layers).Count -eq $count+1} 'Card footer New Layer did not add a layer'
    Invoke "bubble-properties-$layers";Wait-Until {(View $layers).card.properties} 'Card properties did not open'
    Settled-Capture 'layer-properties';Invoke "bubble-properties-$layers";Wait-Until {!(View $layers).card.properties} 'Card properties did not close'
    $card=Card $layers;$b=Card-Bounds $layers $card
    Inspect-InactiveProperties $layers
    $blank=@{x=[int]($b.X+3*$scale);y=[int]($b.Y+$b.Height*.5)}
    [CapyRowPointer]::Down($Device,$blank.x,$blank.y)
    Wait-Until {$null -ne (Find 'Full' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))} 'Holding blank card space did not open its menu'
    [CapyRowPointer]::Up();Invoke 'Full' -Name
    Wait-Until {(View $layers).card.full} 'Full card option did not apply'
    Escape
    $caption=(Model).state.layer_properties.disclosure.label
    Wait-Until {(Control 'layer-properties-disclosure-label' -Within (Card $layers)).Current.Name -eq $caption} 'Full card disclosure did not reuse shared native caption'
    Settled-Capture 'layers-full'
    if($NumericOnly -or $CurveOnly){if($NumericOnly){Native-NumericContacts $layers}else{Native-CurveContacts $layers};@{checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved|Out-Null;return}
    if($NativeControlsOnly -or $ScrollEpochOnly){Layer-ListContacts $layers;@{checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved|Out-Null;return}
    if($Device -eq 'touch'){
        $trials=@()
        foreach($trial in @(@{stage='fast';delay=16},@{stage='fast2';delay=16},@{stage='fast3';delay=16},@{stage='fast4';delay=16},@{stage='fast5';delay=16},@{stage='near';delay=16},@{stage='delayed';delay=150})){
            $result=Layer-Pair $layers $trial.delay $trial.stage;$trials+=@{stage=$trial.stage;result=$result}
            [CapyRowPointer]::Hover($w.X+20,$w.Y+20);Start-Sleep -Milliseconds 150
            $currentOpacity=@((Model).state.layers|Where-Object editing)[0].opacity
            if($currentOpacity -ne $result.expected_opacity){$slider=Control 'layer-opacity-slider' -Within (Card $layers);$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue($result.expected_opacity);Wait-Until {@((Model).state.layers|Where-Object editing)[0].opacity -eq $result.expected_opacity} 'Diagnostic native restore failed'}
        }
        $trials|ConvertTo-Json -Depth 14|Set-Content (Join-Path $run 'layer-pair-trials.json')
        $failed=@($trials|Where-Object {$_.result.actual_opacity -ne $_.result.expected_opacity -or $_.result.recovery_error})
        if($failed.Count){throw ('Native pair trials failed: '+($failed.stage -join ', '))}
        $trials[0].result|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'layer-pair-proof.json')
        $checks.Add('pair-cancels-native-layer-slider')
    }
    $checks.Add('layers-native-inspector-footer-properties')
    Rapid-Reopen $layers
    if($RapidOnly){& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved;@{rapid_transition='passed';checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');return}
    $color=Take 'color' '' ($width*.72) ($height*.40)
    $diameter=(View $color).diameter;$pick=Relative $color (Wheel-FieldPoint $color)
    $old=(Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress
    Tap $pick;Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress) -ne $old} 'Wheel tap did not pick a color'
    if($Device -eq 'touch'){
        $c=Center $color;$distance=[int]($diameter*$scale*.18)
        $pairColor=(Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress
        Touch-Pair ($c.x-$distance) $c.y ($c.x+$distance) $c.y 0 0 1.25
        Wait-Until {(View $color).diameter -gt $diameter} 'Two fingers starting on wheel did not resize ring'
        Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress) -eq $pairColor} 'Second wheel finger did not restore the first finger color pick'
        $checks.Add('wheel-two-finger-pinch')
    }
    $colorBefore=(Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress
    $size=(View $color).diameter;$inside=Relative $color (Wheel-FieldPoint $color);$outside=Relative $color @((-$size*1.6),0)
    $wheelStart=Center $color
    Drag $inside $outside
    Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress) -eq $colorBefore} 'Dragging out of wheel did not restore its original color'
    Wait-Until {$now=Center $color;[Math]::Sqrt([Math]::Pow($now.x-$wheelStart.x,2)+[Math]::Pow($now.y-$wheelStart.y,2)) -gt $size*$scale*.25} 'Dragging out of wheel did not move its center'
    $checks.Add('wheel-drag-out-restores-color')
    Hold-Bubble $color
    $beforeOptions=(View $color).options;$changed=@{}
    foreach($option in @(@{label='Recent colors';key='recent'},@{label='Primary and secondary';key='pair'},@{label='Palette';key='palette'})){
        Invoke $option.label -Name;$key=$option.key;$changed[$key]=!$beforeOptions.$key
        Wait-Until {(View $color).options.$key -eq $changed[$key]} 'Native menu option did not flip its shared value'
        if(!(Find 'Put back' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem))){throw 'Checking an option closed the bubble menu'}
        foreach($prior in $changed.Keys){if((View $color).options.$prior -ne $changed[$prior]){throw 'Later native option changed an earlier choice'}}
    }
    @{before=$beforeOptions;after=(View $color).options}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'option-toggles.json')
    Escape;Wait-Until {@((View $color).beads).Count -gt 3} 'Color options did not publish orbit beads'
    $palette=@((View $color).beads|Where-Object {$_.placement.bead.kind -eq 'palettes'})[0]
    if(!$palette){throw 'Palette bead is missing'}
    Tap (Relative $color $palette.placement.center);Wait-Until {$null -ne (View $color).pill} 'Palette bead did not open palette pill'
    Start-Sleep -Milliseconds 450
    $pill=(Packet $color).pill;$name=(View $color).pill;$center=Center $color;$area=(Model).layout.work_area
    $left=$center.x+$pill.x*$scale;$top=$center.y+$pill.y*$scale;$right=$left+$pill.width*$scale;$bottom=$top+$pill.height*$scale
    if([Math]::Abs($pill.width-184) -gt .05 -or [Math]::Abs($pill.height-30) -gt .05 -or $left -lt $w.X+$area.x*$scale-1 -or $top -lt $w.Y+$area.y*$scale-1 -or $right -gt $w.X+($area.x+$area.width)*$scale+1 -or $bottom -gt $w.Y+($area.y+$area.height)*$scale+1){throw 'Published palette pill does not fit the work area at 184 x 30 px'}
    $orbit=@()
    foreach($bead in @((View $color).beads)){
        $placement=$bead.placement;if($placement.alpha -le 0){continue};$at=Relative $color $placement.center;$radius=$placement.size*$scale*.5
        if($at.x-$radius -lt $w.X+$area.x*$scale-1 -or $at.y-$radius -lt $w.Y+$area.y*$scale-1 -or $at.x+$radius -gt $w.X+($area.x+$area.width)*$scale+1 -or $at.y+$radius -gt $w.Y+($area.y+$area.height)*$scale+1){throw 'Color orbit exceeds the work area'}
        $orbit+=@{kind=$placement.bead.kind;center=$at;size=$placement.size;alpha=$placement.alpha}
    }
    $orbit|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'color-orbit-bounds.json')
    $buttons=@()
    foreach($expected in @(@{x=0;width=32},@{x=34;width=116},@{x=152;width=32})){
        $at=Relative $color @(($pill.x+$expected.x+$expected.width*.5),($pill.y+15))
        $candidates=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))|Where-Object {$b=$_.Current.BoundingRectangle;!$_.Current.IsOffscreen -and $b.Contains([Windows.Point]::new($at.x,$at.y))}|Sort-Object {$b=$_.Current.BoundingRectangle;$b.Width*$b.Height})
        if(!$candidates.Count){@{point=$at;published=$pill;center=$center}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'palette-native-miss.json');throw 'Palette pill lacks its native button'}
        $element=$candidates[0]
        $bounds=$element.Current.BoundingRectangle;$buttons+=@{name=$element.Current.Name;bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);expected=$expected}
        if([Math]::Abs($bounds.Height-30*$scale) -gt 2 -or $bounds.X -lt $left-2 -or $bounds.Right -gt $right+2 -or $bounds.Y -lt $top-2 -or $bounds.Bottom -gt $bottom+2){throw 'Native palette button exceeds its 30 px pill'}
        if([Math]::Abs($bounds.Width-$expected.width*$scale) -gt 1.5 -or [Math]::Abs($bounds.X-($left+$expected.x*$scale)) -gt 1.5){throw "Native palette button differs from GTK width $($expected.width) px at x $($expected.x) px"}
    }
    $gaps=@(0,1|ForEach-Object {($buttons[$_+1].bounds[0]-$buttons[$_].bounds[0]-$buttons[$_].bounds[2])/$scale})
    @{published=$pill;native_buttons=$buttons;gaps=$gaps}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'palette-pill-bounds.json')
    foreach($gap in $gaps){if([Math]::Abs(($gap-2)*$scale) -gt 1.5){throw 'Native palette buttons do not have GTK 2 px gaps'}}
    $palettes=@((Model).state.color_library.palettes);$index=[Array]::IndexOf(@($palettes.name),$name)
    if($index -lt 0 -or $palettes.Count -lt 3){throw 'Palette fixture cannot distinguish exactly one step'}
    $nextName=$palettes[($index+1)%$palettes.Count].name;$next=Relative $color @(($pill.x+$pill.width-16),($pill.y+15))
    Tap $next;Wait-Until {(View $color).pill -eq $nextName} 'One native Next tap did not advance exactly one palette'
    Start-Sleep -Milliseconds 400;if((View $color).pill -ne $nextName){throw 'Next palette changed more than once'}
    $pill=(Packet $color).pill;$previous=Relative $color @(($pill.x+16),($pill.y+15))
    Tap $previous;Wait-Until {(View $color).pill -eq $name} 'One native Previous tap did not restore exactly one palette'
    Start-Sleep -Milliseconds 400;if((View $color).pill -ne $name){throw 'Previous palette changed more than once'}
    Settled-Capture 'palette-pill'
    $checks.Add('persistent-options-beads-palette-pill')
    if($BeadTransitionsOnly){Assert-BubbleVisualBeadTransitions $color}
    Settled-Capture 'color-ring'
    $checks.Add('wheel-pick')
    if($VisualOnly -and !$BeadTransitionsOnly){Assert-BubbleVisualEdges $dial $color}
    $canvas=Control 'drawing-canvas';$canvas.SetFocus();Wait-Until {$canvas.Current.HasKeyboardFocus} 'Canvas did not receive native focus for Zen'
    [CapyRowPointer]::Key([uint32]$review.Id,0x09)
    Wait-Until {(Model).state.workspace.zen_mode} 'Tab did not enter Zen'
    foreach($id in @($dial,$controls,$history,$color)){if(!(Find "tool-bubble-$id" -Visible)){throw "Zen hid bubble $id"}}
    Settled-Capture 'zen';[CapyRowPointer]::Key([uint32]$review.Id,0x09);Wait-Until {!(Model).state.workspace.zen_mode} 'Tab did not leave Zen'
    $checks.Add('zen-retains-bubbles')
    $canvas.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09);Wait-Until {(Model).state.workspace.zen_mode} 'Tab did not reenter Zen'
    Escape;Wait-Until {!(Model).state.workspace.zen_mode} 'Fresh Escape did not leave Zen'
    foreach($id in @($dial,$controls,$history,$color)){if(!(Find "tool-bubble-$id" -Visible)){throw "Leaving Zen hid bubble $id"}}
    $checks.Add('fresh-escape-leaves-zen-and-retains-bubbles')
    if($VisualOnly -or $BeadTransitionsOnly){@{checks=@($checks)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'results.json');& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved|Out-Null;return}
    Document-SwitchMenu $dial
    Retained-Menu $color
    foreach($packet in @(Packets)){
        $id=$packet.view.id
        if($packet.view.kind -eq 'layers' -and $packet.view.card){Tap (Screen ($width*.9) ($height*.8));Wait-Until {$null -eq (View $id).card} 'Outside contact did not fold Layers'}
        $from=if($packet.view.kind -eq 'color'){Relative $id @((-$packet.view.diameter*.5+3),0)}else{Center $id}
        Drag $from (Screen ($width*.5) 12)
        Wait-Until {$null -eq (Packet $id)} 'Title-bar drop did not put bubble back'
    }
    if(@(Packets).Count){throw 'Put back left bubbles behind'}
    Settled-Capture 'put-back'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{variant=$Variant;checks=@($checks);scope='OS-injected input with native pixel and geometry checks; physical input, matched GTK captures and presentation performance unverified'}|ConvertTo-Json -Depth 12|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
    $bubbleFailed=$true
    $CapyEach=$null
    if($review -and !$review.HasExited){
        $diagnostics=@{}
        foreach($id in @('tool-bubble-desk','title-bar','layer-opacity-slider','drawing-canvas')){
            try{$element=Find $id;if($element){$diagnostics[$id]=@{help=$element.Current.HelpText;status=$element.Current.ItemStatus;bounds=$element.Current.BoundingRectangle.ToString()}}}catch{}
        }
        try{
            if($layers){$card=Find "bubble-card-$layers";if($card){$diagnostics['bubble-card']=@{help=$card.Current.HelpText;status=$card.Current.ItemStatus;bounds=$card.Current.BoundingRectangle.ToString()};$nativeList=Find 'layer-list' -Within $card;if($nativeList){$diagnostics['card-layer-list']=@{help=$nativeList.Current.HelpText;status=$nativeList.Current.ItemStatus;bounds=$nativeList.Current.BoundingRectangle.ToString()}};$slider=Find 'layer-opacity-slider' -Within $card;if($slider){$diagnostics['card-opacity-slider']=@{help=$slider.Current.HelpText;bounds=$slider.Current.BoundingRectangle.ToString()}}}}
        }catch{}
        try{
            $named=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Drawing workspace'))
            $diagnostics['drawing-workspaces']=@($named|ForEach-Object {@{id=$_.Current.AutomationId;help=$_.Current.HelpText;status=$_.Current.ItemStatus;bounds=$_.Current.BoundingRectangle.ToString()}})
        }catch{}
        try{$diagnostics['bubble-peers']=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -like 'tool-bubble-*'}|ForEach-Object {@{id=$_.Current.AutomationId;status=$_.Current.ItemStatus;help=$_.Current.HelpText;bounds=$_.Current.BoundingRectangle.ToString()}})}catch{}
        $diagnostics|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'input-diagnostics.json')
    }
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel -Composed}catch{}}
    ($_|Out-String)+$_.ScriptStackTrace|Set-Content (Join-Path $run 'failure.txt')
    if($review -and !$review.HasExited){try{& (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved}catch{}}
    throw
}finally{
    [CapyRowPointer]::Dispose();Exit-CapyEnvironment
    if($HdrOnly){
        try{[CapyCanvasTouch]::Dispose();if($review){$review.Refresh();if(!$review.HasExited){$review.Kill();if(!$review.WaitForExit(8000)){throw 'Owned HDR bubble app did not close'}}}}catch{if(!$bubbleFailed){throw};Write-Warning $_}
        try{$hdrEvidence|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run 'hdr-evidence.json')}catch{if(!$bubbleFailed){throw};Write-Warning $_}
    }
}
