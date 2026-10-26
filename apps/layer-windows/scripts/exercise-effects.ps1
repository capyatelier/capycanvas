param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','curve-dark','curve-light')][string]$Theme='dark',[switch]$PropertyLayout)
$ErrorActionPreference='Stop'
$curveOnly=$Theme.StartsWith('curve-');if($curveOnly){$Theme=$Theme.Substring(6)}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true;$CapyCaptureDelay=250
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
public static class CapyEffectsCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/effects/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Assert-NumericReadoutFit($Element,[double]$Scale){
    Add-Type -AssemblyName PresentationCore
    $range=$Element.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).DocumentRange
    $font=$range.GetAttributeValue([System.Windows.Automation.TextPattern]::FontNameAttribute)
    $points=$range.GetAttributeValue([System.Windows.Automation.TextPattern]::FontSizeAttribute)
    if($font -isnot [string] -or $points -isnot [double] -or $points -le 0 -or $Scale -le 0){throw 'Numeric readout font or scale metadata is unavailable'}
    $value=$Element.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    $measured=[Windows.Media.FormattedText]::new($value,[Globalization.CultureInfo]::CurrentCulture,[Windows.FlowDirection]::LeftToRight,[Windows.Media.Typeface]::new($font),($points*96/72),[Windows.Media.Brushes]::Black,$Scale)
    $required=($measured.WidthIncludingTrailingWhitespace+12)*$Scale
    $width=$Element.Current.BoundingRectangle.Width
    if($required -gt $width+1){throw "Numeric readout '$value' needs $required physical pixels including padding; available width is $width"}
}
function Invoke([string]$Value,[switch]$Name){
    $hit=@{item=$null}
    Wait-Until {
        $hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::Button)
        if(!$hit.item){$hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $null -ne $hit.item
    } "Missing action: $Value"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Open-Preferences{
    if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{
        $caption=@((Model).application_menus|Where-Object id -eq 'edit')[0].label
        if(!$caption){throw 'Current shared application menu is missing: edit'}
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit' -Caption $caption
        Invoke 'settings'
    }
    Wait-Until {(Model).state.settings_open} 'Preferences action was not acknowledged'
    Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window) -Arranged|Out-Null
}
function Edit([string]$Id,[string]$Text){
    $entry=Control $Id;$entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} "Numeric editor did not receive focus: $Id"
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
    Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Text} "Numeric editor did not receive text: $Id"
}
function Edit-Value([string]$Id,[string]$Text){
    Invoke $Id;Edit ($Id+'-entry') $Text;[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    Wait-Until {!(Find ($Id+'-entry'))} "Edit Color did not commit $Id"
}
function Edit-Form([int]$Row,[string]$Form){
    Invoke "edit-color-form-$Row";$item=Control "edit-color-form-$Row-$Form";$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    elseif($item.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)){$pattern.Select()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
    Wait-Until {$item=Find "edit-color-form-$Row-$Form";!$item -or $item.Current.IsOffscreen} 'The format menu did not close'
}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object {$_.key -eq $Key}}
function Rgba($Color){if($null -ne $Color.rgba){$Color.rgba}else{$Color}}
function Choose([string]$Id,[string]$Option){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Select-Panel([string]$Id){
    if(@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -eq 0){Invoke $(if(Find ('drawer-tab-'+$Id)){'drawer-tab-'+$Id}else{'panel-tab-'+$Id})}
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq $Id}).Count -gt 0} "Panel $Id did not become active"
}
function Select-Filter([string]$Id,[string]$Label){
    Select-Panel 'adjustments'
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'adjustments'}).Count -gt 0} 'Filters tab did not open'
    if($null -ne (Model).state.filter_picker.search){Invoke 'filter-search-toggle'}
    Choose 'filter-category' 'All filters'
    Invoke 'filter-search-toggle'
    Edit 'filter-search' $Label
    Wait-Until {(Model).state.filter_picker.search -eq $Label} 'Search not acknowledged'
    Wait-Until {
        $buttons=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
        $rows=@($buttons|Where-Object {$_.Current.AutomationId.StartsWith('filter-') -and $_.Current.AutomationId -ne 'filter-search-toggle'})
        $rows.Count -eq (Model).state.adjustments.Count -and $null -ne (Find ('filter-'+$Id))
    } 'Search rows did not reach the native tree'
    Invoke ('filter-'+$Id)
    Wait-Until {(Model).state.layer_properties.description -eq $Label} 'Inserted filter did not become active'
    Select-Panel 'properties'
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'properties'}).Count -gt 0} 'Properties did not appear'
}
function Preview-Hash([string]$Id){
    $image=Control ('filter-preview-'+$Id)
    if($image.Current.ItemStatus -ne 'Ready'){return ''}
    $r=$image.Current.BoundingRectangle
    $window=[CapyEffectsCapture+Rect]::new()
    if(![CapyEffectsCapture]::GetWindowRect($drawingWindow,[ref]$window)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($window.right-$window.left,$window.bottom-$window.top)
    try{
        $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
        try{if(![CapyEffectsCapture]::PrintWindow($drawingWindow,$dc,2)){throw 'App capture failed'}}
        finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
        $area=[Drawing.Rectangle]::new([int]$r.X-$window.left+2,[int]$r.Y-$window.top+2,[int]$r.Width-4,[int]$r.Height-4)
        $crop=$bitmap.Clone($area,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stream=[IO.MemoryStream]::new();$sha=[Security.Cryptography.SHA256]::Create()
        try{$crop.Save($stream,[Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($sha.ComputeHash($stream.ToArray()))}
        finally{$crop.Dispose();$stream.Dispose();$sha.Dispose()}
    }finally{$bitmap.Dispose()}
}

function Field([string]$Axis){Control "property-rgb-$Axis"}
function Finite([double]$Value){![double]::IsNaN($Value) -and ![double]::IsInfinity($Value)}
function Graph-Height{200*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.}
function Show-Graph([double]$Percent=0){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent((Control 'property-rgb-curve'))
    while($node){$scroll=$null;if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable){break};$node=$walker.GetParent($node)}
    if($node -and $scroll.Current.VerticalScrollPercent -ne $Percent){$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$Percent)}
    $settled=@{bounds=$null;scroll=-1;visible=$false;enabled=$false}
    try{Wait-Until {
        $graph=Control 'property-rgb-curve';$bounds=$graph.Current.BoundingRectangle
        $visible=!$bounds.IsEmpty -and (Finite $bounds.X) -and (Finite $bounds.Y) -and (Finite $bounds.Width) -and (Finite $bounds.Height) -and $bounds.Width -gt 0 -and $bounds.Height -gt (6*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.) -and !$graph.Current.IsOffscreen
        $same=$visible -and $graph.Current.IsEnabled -and $bounds -eq $settled.bounds
        if(!$visible -and $node -and $scroll.Current.VerticallyScrollable){
            $currentPercent=$scroll.Current.VerticalScrollPercent
            if($Percent -ge 50 -and $currentPercent -gt 0){$scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::SmallDecrement)}
            elseif($Percent -lt 50 -and $currentPercent -lt 100){$scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::SmallIncrement)}
        }
        $settled.bounds=$bounds;$settled.visible=$visible;$settled.enabled=$graph.Current.IsEnabled;if($scroll){$settled.scroll=$scroll.Current.VerticalScrollPercent}
        Start-Sleep -Milliseconds 100;$same
    } 'The visible curve graph did not settle after scrolling'}
    catch{throw "Curve graph did not settle: bounds=$($settled.bounds), scroll=$($settled.scroll), visible=$($settled.visible), enabled=$($settled.enabled)"}
}
function Tap-Point([int]$Index){
    $point=(Property 'rgb').value.value[$Index];$low=$point[1] -lt .5;Show-Graph $(if($low){100}else{0})
    $graph=Control 'property-rgb-curve';$r=$graph.Current.BoundingRectangle;$height=Graph-Height;$top=if($low){$r.Bottom-$height}else{$r.Y}
    $margin=3*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $at=@([int][Math]::Min([Math]::Max($r.X+$point[0]*$r.Width,$r.Left+$margin),$r.Right-$margin),[int][Math]::Min([Math]::Max($top+(1-$point[1])*$height,$r.Top+$margin),$r.Bottom-$margin))
    $hit=[System.Windows.Automation.AutomationElement]::FromPoint([System.Windows.Point]::new($at[0],$at[1]))
    $native=$hit.Current.AutomationId
    if(!$hit -or $hit.Current.ProcessId -ne $review.Id -or $graph.Current.IsOffscreen -or !$graph.Current.IsEnabled -or !$r.Contains([System.Windows.Point]::new($at[0],$at[1]))){throw "Curve point $Index misses the owned visible graph: at=$at bounds=$r native=$native points=$((Property 'rgb').value.value|ConvertTo-Json -Compress)"}
    [CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up()
    Wait-Until {(Property 'rgb').curve.selected -eq $Index} "Tapping point $Index did not select it: at=$at bounds=$r native=$native"
    Wait-Until {$graph.Current.HasKeyboardFocus} "Tapping curve point $Index did not focus the graph"
}
function Pointer-Session([scriptblock]$Body){
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    try{& $Body}catch{
        try{Capture 'pointer-failure-before-cleanup' -WithModel -Composed}catch{}
        throw
    }finally{[CapyRowPointer]::Dispose()}
}
function Stops{@((Property 'gradient').value.value.stops)}
function Gradient-Json{(Property 'gradient').value.value|ConvertTo-Json -Depth 10 -Compress}
function Near-Stop([double]$Position){@(Stops|Where-Object {[Math]::Abs($_.position-$Position) -lt .02}).Count -gt 0}
function Strip-Point([double]$Fraction){
    $r=(Control 'property-gradient-gradient').Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    @([int]($r.X+6*$scale+($r.Width-12*$scale)*$Fraction),[int]($r.Y+16*$scale))
}
function Strip-Drag([string]$Device,[double]$From,[double]$To,[switch]$Escape){
    $start=Strip-Point $From;$end=Strip-Point $To
    [CapyRowPointer]::Down($Device,$start[0],$start[1])
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($start[0]+($end[0]-$start[0])*$i/8),$start[1]);Start-Sleep -Milliseconds 30}
    if($Escape){[CapyRowPointer]::Key(0x1B)}
    [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
}
function Check-Undo([string]$Before,[string]$Reason){
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $Before} "$Reason was not one Undo"
    Invoke 'Redo' -Name;Wait-Until {(Gradient-Json) -ne $Before} "$Reason did not redo"
}
function Check-Gradient {Pointer-Session {
    if((Stops).Count -ne 2){throw 'Gradient Map did not start with two stops'}
    foreach($pass in @(@('mouse',.3,.55),@('touch',.7,.85),@('pen',.15,.4))){
        $before=Gradient-Json;$count=(Stops).Count
        Strip-Drag $pass[0] $pass[1] $pass[2]
        Wait-Until {(Stops).Count -eq $count+1 -and (Near-Stop $pass[2])} "$($pass[0]) did not add and drag a gradient stop"
        Check-Undo $before "$($pass[0]) add and drag"
    }
    $before=Gradient-Json
    Strip-Drag 'mouse' .55 .7 -Escape
    Wait-Until {(Gradient-Json) -eq $before} 'Escape did not cancel the stop drag'
    (Control 'property-gradient-gradient').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x27)
    Wait-Until {(Near-Stop .65) -and !(Near-Stop .55)} 'Shift+Right did not move the selected stop by ten steps'
    Check-Undo $before 'A keyboard step'
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $before} 'Keyboard step did not undo'
    (Control 'property-gradient-gradient').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x2E)
    Wait-Until {(Stops).Count -eq 4} 'Delete did not remove the selected stop'
    Check-Undo $before 'Delete'
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $before} 'Delete did not undo'
    $mode=@((Property 'gradient').gradient.interpolations)[1]
    Choose 'property-gradient-interpolation' $mode[1]
    Wait-Until {(Property 'gradient').value.value.interpolation -eq $mode[0]} 'Interpolation did not apply'
    Check-Undo $before 'Interpolation'
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $before} 'Interpolation did not undo'
    $positions=@(Stops|ForEach-Object position)
    Invoke 'property-gradient-reverse'
    Wait-Until {$after=@(Stops|ForEach-Object position);$mirrored=$true;for($i=0;$i -lt $positions.Count;$i++){if([Math]::Abs($after[$i]-(1-$positions[$positions.Count-1-$i])) -gt .000001){$mirrored=$false}};$mirrored} 'Reverse did not mirror the stops'
    Check-Undo $before 'Reverse'
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $before} 'Reverse did not undo'
    Invoke 'property-gradient-use-color'
    Wait-Until {(Gradient-Json) -ne $before} 'Use current color did not change the selected stop'
    Check-Undo $before 'Use current color'
    Invoke 'Undo' -Name;Wait-Until {(Gradient-Json) -eq $before} 'Use current color did not undo'
    $at=Strip-Point .4;[CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
    if((Gradient-Json) -ne $before){throw 'Selecting a stop changed the gradient'}
}}
function Check-PropertyScrub {Pointer-Session {
    $opacity=(Property 'opacity').value.value;$track=(Control 'property-opacity-slider').Current.BoundingRectangle;$y=[int]($track.Y+$track.Height/2)
    [CapyRowPointer]::Down('mouse',[int]($track.X+$track.Width*.3),$y)
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($track.X+$track.Width*(.3+.05*$i)),$y);Start-Sleep -Milliseconds 30}
    [CapyRowPointer]::Up()
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Opacity slider release did not finish its gesture'
    Wait-Until {[Math]::Abs((Property 'opacity').value.value-.7) -lt .03} 'The opacity slider did not follow the drag'
    Invoke 'Undo' -Name;Wait-Until {(Property 'opacity').value.value -eq $opacity} 'An opacity slider drag needed more than one Undo'
}}
function Property-Row([string]$Key,[string]$Case){
    $property=Property $Key
    if($property.kind.kind -ne 'number' -or $property.kind.numeric.kind -ne 'slider'){throw "Expected a shared slider property: $Key"}
    $panel=Control 'layer-properties' -Arranged
    $caption=Control $property.label -Name -Within $panel -Type ([System.Windows.Automation.ControlType]::Text) -Arranged
    $field=Control ('number-root-property-'+$Key) -Within $panel -Arranged
    $entry=Control ('property-'+$Key) -Within $field -Arranged
    $track=Control ('property-'+$Key+'-slider') -Within $field -Arranged
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $track.SetFocus();Wait-Until {!$entry.Current.HasKeyboardFocus} 'Property editor did not return to readout'
    $readout=Native-Bounds ('property-'+$Key) $scale
    Capture $Case -Composed
    $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'Property editor did not receive focus for geometry'
    $bounds=@{panel=$panel.Current.BoundingRectangle;caption=(Native-Bounds ('number-caption-property-'+$Key) $scale);field=(Native-Bounds ('number-root-property-'+$Key) $scale);entry=(Native-Bounds ('property-'+$Key) $scale);track=(Native-Bounds ('property-'+$Key+'-slider') $scale)}
    @{case=$Case;key=$Key;label=$property.label;bounds=$bounds;value=$property.value;dpi=96*$scale;identities=@{entry=($entry.GetRuntimeId() -join ':');track=($track.GetRuntimeId() -join ':')}}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ($Case+'-geometry.json'))
    @{readout=$readout;editing=$bounds.entry;dpi=96*$scale}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ($Case+'-editing-geometry.json'))
    Capture ($Case+'-editing') -Composed
    $outer=[Windows.Rect]::new($bounds.panel.X-1,$bounds.panel.Y-1,$bounds.panel.Width+2,$bounds.panel.Height+2);$inner=[Windows.Rect]::new($bounds.field.X-1,$bounds.field.Y-1,$bounds.field.Width+2,$bounds.field.Height+2)
    if(!$outer.Contains($bounds.caption) -or !$outer.Contains($bounds.field) -or !$inner.Contains($bounds.entry) -or !$inner.Contains($bounds.track) -or !$inner.Contains($readout)){throw "Property controls are outside their visible panel: $Key"}
    if([Math]::Abs($bounds.field.Height-36*$scale) -gt 1 -or [Math]::Abs($bounds.entry.Width-68*$scale) -gt 1 -or [Math]::Abs($bounds.entry.Height-26*$scale) -gt 1){throw "Compact property row or value dimensions differ: $Key"}
    if([Math]::Abs($bounds.caption.Left-$bounds.field.Left) -gt 1 -or [Math]::Abs($bounds.entry.Right-$bounds.field.Right) -gt 1){throw "Compact property label or value has an extra inset: $Key"}
    if([Math]::Abs($bounds.entry.Bottom-$bounds.field.Bottom) -gt 1 -or [Math]::Abs($bounds.track.Bottom-$bounds.field.Bottom) -gt 1 -or [Math]::Abs($readout.Right-$bounds.entry.Right) -gt 1 -or [Math]::Abs($readout.Bottom-$bounds.entry.Bottom) -gt 1 -or [Math]::Abs($readout.Height-$bounds.entry.Height) -gt 1){throw "Compact property readout/editor bottom or right alignment differs: $Key"}
    $controls=@((Model).state.layer_properties.controls)
    $position=[array]::IndexOf(@($controls|ForEach-Object key),$Key)
    if($position -ge 0 -and $position+1 -lt $controls.Count -and $controls[$position+1].kind.kind -eq 'number' -and $controls[$position+1].section -eq $property.section){
        $next=Find ('number-root-property-'+$controls[$position+1].key)
        if($next -and !$next.Current.IsOffscreen -and [Math]::Abs(((Native-Bounds ('number-root-property-'+$controls[$position+1].key) $scale).Top-$bounds.field.Bottom)/$scale-4) -gt 1){throw "Related Properties numeric rows do not share the compact gap: $Key"}
    }
    $origin=$bounds.field.Left
    if([Math]::Abs($bounds.track.Height-16*$scale) -gt 1 -or [Math]::Abs($bounds.track.Left-$origin-36*$scale) -gt 1 -or [Math]::Abs($bounds.field.Right-$bounds.track.Right-76*$scale) -gt (2*$scale+1) -or $bounds.track.Width -lt 80*$scale){throw "Compact property slider dimensions differ: $Key"}
    $client=[CapyWindowCapture+Point]::new()
    if(![CapyWindowCapture]::ClientToScreen($drawingWindow,[ref]$client)){throw 'Property capture origin unavailable'}
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Case+'-editing.png')))
    try{
        $y=[int]($bounds.track.Top+$bounds.track.Height/2-$client.y)
        $background=$bitmap.GetPixel([int]($bounds.track.Left-$client.x-4*$scale),$y).ToArgb()
        $pixels=@([int]($bounds.track.Left-$client.x)..[int]($bounds.track.Right-$client.x)|Where-Object {$bitmap.GetPixel($_,$y).ToArgb() -ne $background})
        if(!$pixels.Count){throw 'Visible property slider pixels were absent'}
        $visibleLeft=$pixels[0]+$client.x;$visibleRight=$pixels[-1]+$client.x+1
        @{origin=$origin;left=$visibleLeft;right=$visibleRight;editor_left=$bounds.entry.Left;editor_right=$bounds.entry.Right;scale=$scale}|ConvertTo-Json|Set-Content (Join-Path $run ($Case+'-track-pixels.json'))
        if([Math]::Abs($visibleLeft-[Math]::Round($origin+36*$scale)) -gt 1 -or [Math]::Abs($visibleRight-[Math]::Round($bounds.field.Right-76*$scale)) -gt 1 -or [Math]::Abs($visibleRight-[Math]::Round($bounds.entry.Left-8*$scale)) -gt 1){throw "Visible property track insets or editor gap differ: $Key"}
    }finally{$bitmap.Dispose()}
    if($bounds.caption.Right -gt $bounds.entry.Left+1 -or $bounds.track.Right -gt $bounds.entry.Left+1 -or $bounds.caption.Bottom -gt $bounds.track.Top+1){throw "Compact property caption, slider or value overlap: $Key"}
    (Control 'drawing-canvas').SetFocus()
    Wait-Until {!$entry.Current.HasKeyboardFocus} 'The compact editor did not finish its unchanged draft'
}
function Property-Resize([int]$Width){
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]($Width*$scale)) -Height ([int](900*$scale))
    Wait-Until {[Math]::Abs($root.Current.BoundingRectangle.Width-$Width*$scale) -lt 2} 'Properties window did not acknowledge its dimensions'
}
function Property-Language([string]$Tag){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'edit'
    Invoke 'settings';Invoke 'preference-page-appearance'
    $statePath=State-File
    $tags=@((Read-Snapshot (Join-Path (Split-Path -Parent $statePath) ((Split-Path -Leaf $statePath) -replace '^ui-state-','bootstrap-'))).shipped_tags)
    $index=1+[Array]::IndexOf($tags,$Tag)
    if($index -lt 1){throw "Missing shared locale: $Tag"}
    $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
    Choose 'preference-choice-language' $row.kind.options[$index]
    Wait-Until {(Model).windows_active_tag -eq $Tag} 'Properties language choice was not acknowledged'
    Invoke 'CloseButton';Wait-Until {!(Model).preferences -and (Control 'drawing-canvas').Current.IsEnabled} 'Properties language preferences did not close'
}
function Property-Typed([string]$Key,[string]$Text,[double]$Expected){
    $before=(Property $Key).value.value
    Edit ('property-'+$Key) $Text;(Control ('property-'+$Key+'-slider')).SetFocus()
    Wait-Until {[Math]::Abs((Property $Key).value.value-$Expected) -lt 1e-6} "Typed property did not commit: $Key"
    Invoke 'Undo' -Name;Wait-Until {(Property $Key).value.value -eq $before} "Typed property needed more than one Undo: $Key"
}
function Property-Theme {
    $before=(Model).state.theme
    Open-Preferences
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=if($before -eq 'dark'){'Light'}else{'Dark'}
    (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $before} 'Property theme change not acknowledged'
    Invoke 'CloseButton'
    Wait-Until {!(Model).preferences} 'Property theme preferences did not close'
}
function Check-PropertyValueScrub {Pointer-Session {
    foreach($device in @('pen','mouse','touch')){
        Edit 'property-opacity' '50';(Control 'property-opacity-slider').SetFocus()
        Wait-Until {[Math]::Abs((Property 'opacity').value.value-.5) -lt 1e-6} 'Value scrub seed did not commit'
        try{Wait-Until {(Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -in @('50','50 %') -and [Math]::Abs((Control 'property-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value-.5) -lt 1e-6} 'Scrub origin did not reach native controls'}catch{throw "Scrub origin native text=$((Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value), slider=$((Control 'property-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value): $_"}
        Write-Host "$device native seed acknowledged"
        if($device -eq 'mouse'){
            $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
            $readout=Native-Bounds 'property-opacity' $scale
            $track=Native-Bounds 'property-opacity-slider' $scale
            $x=[int][Math]::Round($readout.Right-3*$scale);$y=[int][Math]::Round($readout.Top+$readout.Height/2)
            $point=[Windows.Point]::new($x,$y)
            if(!$readout.Contains($point) -or $track.Contains($point) -or $x-$track.Right -lt 2*$scale){throw 'Secondary input does not target the panel value readout'}
            $entry=Control 'property-opacity';$value=(Property 'opacity').value.value
            $text=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
            $focus=([System.Windows.Automation.AutomationElement]::FocusedElement).GetRuntimeId() -join ':'
            if($entry.Current.HasKeyboardFocus){throw 'Secondary readout test started inside the numeric editor'}
            $evidence=@{before=@{value=$value;text=$text;focus=$focus};hit=(Input-Trace $x $y);readout=$readout;slider=$track;device='mouse';button='right'}
            [CapyRowPointer]::RightDrag($x,$y,$x,[int][Math]::Round($y-32*$scale))
            Start-Sleep -Milliseconds 350
            $evidence.after=@{text=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value;editor_focused=$entry.Current.HasKeyboardFocus;focus=(([System.Windows.Automation.AutomationElement]::FocusedElement).GetRuntimeId() -join ':');value=(Property 'opacity').value.value}
            $evidence.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds
            $evidence|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'property-secondary-input.json')
            if($evidence.after.value -ne $value -or $evidence.after.text -cne $text -or $evidence.after.editor_focused -or $evidence.after.focus -cne $focus){throw 'Secondary mouse drag changed the panel value, editor or focus'}
        }
        $before=(Property 'opacity').value.value;$box=(Control 'property-opacity' -Arranged).Current.BoundingRectangle
        $x=[int]($box.Left+$box.Width/2);$y=[int]($box.Top+$box.Height/2)
        $steps=31/([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)/4
        $coarse=$expected=[Math]::Floor($before*100+$steps+.5)/100
        $scrubText=($expected*100).ToString('0.#',[Globalization.CultureInfo]::InvariantCulture)+' %'
        [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 60
        for($step=1;$step -le 6;$step++){[CapyRowPointer]::Move($x,$y-5*$step-1);Start-Sleep -Milliseconds 35}
        try{Wait-Until {[Math]::Abs((Property 'opacity').value.value-$expected) -lt 1e-6 -and (Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $scrubText} "$device value scrub did not preview exact $expected with readout $scrubText"}catch{throw "$device scrub actual=$((Property 'opacity').value.value), native=$((Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value): $_"}
        Write-Host "$device coarse live=$((Property 'opacity').value.value), expected=$expected"
        if($device -eq 'mouse'){
            $fine=[Math]::Round($expected+$steps/10/100,3);$fineText=($fine*100).ToString('0.#',[Globalization.CultureInfo]::InvariantCulture)+' %'
            [CapyRowPointer]::Hold(0x10,$true)
            try{
                for($step=7;$step -le 12;$step++){[CapyRowPointer]::Move($x,$y-5*$step-2);Start-Sleep -Milliseconds 35}
                try{Wait-Until {[Math]::Abs((Property 'opacity').value.value-$fine) -lt 1e-6 -and (Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $fineText} "Shift did not continue the scrub ten times slower from $expected to $fine"}catch{throw "Shift scrub actual=$((Property 'opacity').value.value), native=$((Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value): $_"}
                Capture "opacity-fine-scrub-$((Model).state.theme)" -Composed
                Assert-NumericReadoutFit (Control 'layer-opacity') ([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)
                [CapyRowPointer]::Up()
            }finally{[CapyRowPointer]::Hold(0x10,$false)}
            $expected=$fine
        }else{[CapyRowPointer]::Up()}
        Start-Sleep -Milliseconds 250
        Write-Host "$device coarse after Up=$((Property 'opacity').value.value), native=$((Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value)"
        try{Wait-Until {[Math]::Abs((Property 'opacity').value.value-$expected) -lt 1e-6} "$device coarse release changed the value"}catch{throw "$device coarse release actual=$((Property 'opacity').value.value), expected=${expected}: $_"}
        Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} "$device value scrub did not finish"
        Invoke 'Undo' -Name;Wait-Until {(Property 'opacity').value.value -eq $before} "$device value scrub needed more than one Undo"
        $expected=$coarse
        try{Wait-Until {(Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -in @('50','50 %') -and [Math]::Abs((Control 'property-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value-.5) -lt 1e-6} 'Scrub origin did not reach native controls'}catch{throw "Scrub origin native text=$((Control 'property-opacity').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value), slider=$((Control 'property-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value): $_"}
        [CapyRowPointer]::Down($device,$x,$y);Start-Sleep -Milliseconds 60
        for($step=1;$step -le 6;$step++){[CapyRowPointer]::Move($x,$y-5*$step-1);Start-Sleep -Milliseconds 35}
        Wait-Until {[Math]::Abs((Property 'opacity').value.value-$expected) -lt 1e-6} "$device canceled value scrub did not preview exact $expected"
        Write-Host "$device scrub Escape before=$before, preview=$((Property 'opacity').value.value)"
        [CapyRowPointer]::Key(0x1B)
        Wait-Until {(Property 'opacity').value.value -eq $before} "$device Escape did not cancel while the contact remained held"
        Write-Host "$device scrub after held Escape=$((Property 'opacity').value.value)"
        [CapyRowPointer]::Up()
        Start-Sleep -Milliseconds 250
        Wait-Until {(Property 'opacity').value.value -eq $before} "$device Escape did not cancel the value scrub"
    }
}}
function Check-PropertyLabelReset {Pointer-Session {
        $before=(Property 'opacity').value.value
        Edit 'property-opacity' 'invalid'
        $caption=Control (Property 'opacity').label -Name -Within (Control 'layer-properties') -Type ([System.Windows.Automation.ControlType]::Text) -Arranged
        $box=$caption.Current.BoundingRectangle;$x=[int]($box.Left+$box.Width/2);$y=[int]($box.Top+$box.Height/2)
        for($tap=0;$tap -lt 2;$tap++){[CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 60}
        Wait-Until {(Property 'opacity').value.value -eq 1} 'Double label reset did not discard the invalid numeric draft'
        Invoke 'Undo' -Name;Wait-Until {(Property 'opacity').value.value -eq $before} 'Double label reset needed more than one Undo'
    }}
function Check-PixelValueScrub {Pointer-Session {
    if(!@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count){
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
        $menu=(Model).application_menus|Where-Object id -eq 'window'
        $item=$menu.model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.panel -eq 'tool_settings'}
        (Control $item.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count -gt 0} 'Tool panel did not open'
    }
    Select-Panel 'tool_settings'
    Wait-Until {$null -ne (Find 'tool-setting-size')} 'Brush size field did not appear'
    $value={((Model).state.tool_settings|Where-Object id -eq 'size').value}
    foreach($case in @(@{origin=1200.5;motion=8},@{origin=1200.5;motion=-8},@{origin=31.5;motion=8},@{origin=31.5;motion=-8},@{origin=30;motion=8})){
        Edit 'tool-setting-size' ($case.origin.ToString([Globalization.CultureInfo]::InvariantCulture))
        (Control 'tool-setting-size-slider').SetFocus()
        Wait-Until {(& $value) -eq $case.origin} 'Typed pixel decimal was not retained'
        $native=(Control 'tool-setting-size').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
        Wait-Until {$native.Current.Value -eq ($case.origin.ToString([Globalization.CultureInfo]::InvariantCulture)+' px')} 'Typed pixel decimal readout was not retained'
        Capture ('pixel-'+$case.origin+'-'+$case.motion+'-typed') -Composed
        $bounds=(Control 'tool-setting-size' -Arranged).Current.BoundingRectangle
        $x=[int]($bounds.Right-12);$y=[int]($bounds.Top+$bounds.Height/2)
        $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        $endY=[int]($y-$case.motion*$scale)
        $expected=$case.origin+($y-$endY)/$scale/4
        $expected=if($expected -le 32){[Math]::Floor($expected*10+.5)/10}else{[Math]::Floor($expected+.5)}
        $text=$expected.ToString('0.#',[Globalization.CultureInfo]::InvariantCulture)+' px'
        [CapyRowPointer]::Down('mouse',$x,$y);Start-Sleep -Milliseconds 60
        [CapyRowPointer]::Move($x,$endY)
        try{Wait-Until {(& $value) -eq $expected -and $native.Current.Value -eq $text} 'Pixel scrub precision or readout differed'}catch{throw "Pixel origin=$($case.origin), expected=$expected/$text, actual=$(& $value)/$($native.Current.Value), bounds=$bounds : $_"}
        $after=(Control 'tool-setting-size').Current.BoundingRectangle
        if([Math]::Abs($after.Right-$bounds.Right) -gt 1){throw 'Pixel suffix right edge moved during scrub'}
        Capture ('pixel-'+$case.origin+'-'+$case.motion+'-scrub') -Composed
        [CapyRowPointer]::Key(0x1B)
        Wait-Until {(& $value) -eq $case.origin} 'Escape did not restore the typed pixel decimal while held'
        [CapyRowPointer]::Up()
        Wait-Until {(& $value) -eq $case.origin -and $native.Current.Value -eq ($case.origin.ToString([Globalization.CultureInfo]::InvariantCulture)+' px')} 'Pixel release changed the canceled typed decimal'
        @{origin=$case.origin;motion=$case.motion;expected=$expected;readout=$text;right_before=$bounds.Right;right_after=$after.Right}|ConvertTo-Json|Add-Content (Join-Path $run 'pixel-scrub.jsonl')
    }
}}
function Check-PropertyLayout {
    Check-PixelValueScrub
    Property-Resize 960;Select-Panel 'properties'
    Property-Row 'opacity' 'opacity-compact-en'
    Check-PropertyValueScrub
    Check-PropertyLabelReset
    Property-Theme
    Property-Row 'opacity' 'opacity-compact-alternate-theme'
    Check-PropertyValueScrub
    Check-PropertyLabelReset
    Property-Theme
    $blend=Control 'property-blend' -Arranged;$panel=(Control 'layer-properties').Current.BoundingRectangle
    if(!$panel.Contains($blend.Current.BoundingRectangle)){throw 'Blend is not fully visible below the inline opacity row'}
    Check-PropertyScrub
    Property-Typed 'opacity' '60' .6
    $document=(Model).state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=(Model).windows_gpu_generation
    $entryId=(Control 'property-opacity').GetRuntimeId() -join ':';$blendId=(Control 'property-blend').GetRuntimeId() -join ':';$blendValue=(Property 'blend').value.value
    $trackId=(Control 'property-opacity-slider').GetRuntimeId() -join ':';$opacityValue=(Property 'opacity').value.value
    $retained={((Control 'property-opacity').GetRuntimeId() -join ':') -eq $entryId -and ((Control 'property-opacity-slider').GetRuntimeId() -join ':') -eq $trackId -and (Property 'opacity').value.value -eq $opacityValue -and ((Control 'property-blend').GetRuntimeId() -join ':') -eq $blendId -and (Property 'blend').value.value -eq $blendValue -and ((Model).state.document_file|ConvertTo-Json -Depth 20 -Compress) -eq $document -and (Model).windows_gpu_generation -eq $gpu}
    Property-Resize 744;Property-Language 'ru'
    Property-Row 'opacity' 'opacity-narrow-ru'
    if(!(& $retained)){throw 'Narrow localized Properties changed retained controls, values or document/GPU ownership'}
    Property-Language 'en';Property-Resize 1200
    Property-Row 'opacity' 'opacity-restored-en'
    if(!(& $retained)){throw 'Restored English Properties changed retained controls, values or document/GPU ownership'}
    Select-Panel 'adjustments'
    Compact-Choice 'filter-category' ([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)|Out-Null
    Capture 'filter-category-compact'
    Select-Filter 'color_balance' 'Color Balance'
    $page=@((Model).state.layer_properties.pages|Where-Object id -eq 'midtones')[0]
    Choose 'properties-page' $page.label
    Wait-Until {(Model).state.layer_properties.page -eq 'midtones' -and (Find 'property-midtones_red')} 'Color Balance midtones did not appear'
    Compact-Choice 'properties-page' ([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)|Out-Null
    Property-Row 'midtones_red' 'filter-inline-en'
    Property-Typed 'midtones_red' '12' 12
    Pointer-Session {
    $before=(Property 'midtones_red').value.value;$track=Control 'property-midtones_red-slider' -Arranged;$box=$track.Current.BoundingRectangle;$y=[int]($box.Top+$box.Height/2)
    [CapyRowPointer]::Down('mouse',[int]($box.Left+$box.Width*.3),$y)
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($box.Left+$box.Width*(.3+.05*$i)),$y);Start-Sleep -Milliseconds 30}
    Wait-Until {[Math]::Abs($track.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value-.7) -lt .03 -and (Property 'midtones_red').value.value -ne $before} 'Filter slider did not preview the live drag'
    [CapyRowPointer]::Up()
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Filter slider release did not finish its gesture'
    $edited=(Property 'midtones_red').value.value
    Invoke 'Undo' -Name;Wait-Until {(Property 'midtones_red').value.value -eq $before} 'Filter slider drag needed more than one Undo'
    Invoke 'Redo' -Name;Wait-Until {(Property 'midtones_red').value.value -eq $edited} 'Filter slider drag did not redo exactly'
    Capture 'filter-drag-redo' -Composed
}}

function Check-LogCurve {Pointer-Session {
    $grown=$false
    for($attempt=0;$attempt -lt 5 -and !$grown;$attempt++){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width 1550 -Height 1400
        try{Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Resize pending' 3;$grown=$true}catch{}
    }
    if(!$grown){throw 'The window did not grow for the curve check'}
    Select-Filter 'curves' 'Curves'
    Wait-Until {(Property 'domain').kind.options.Count -eq 2 -and (Find 'property-domain').Current.IsEnabled} 'Curve space did not reach the enabled native control'
    Choose 'property-domain' (Property 'domain').kind.options[1]
    Wait-Until {(Property 'rgb').curve.domain.kind -eq 'log_hdr' -and (Find 'property-hdr_stops')} 'Log HDR did not offer its stops'
    Tap-Point 0
    Wait-Until {$ev=Find 'property-rgb-output-ev';$ev -and $ev.Current.Name -and $ev.Current.Name -eq (Property 'rgb').curve.output.ev} 'Log HDR did not show the EV readout'
    Capture 'curve-log-hdr'
}}

function Check-CurveGestures {
    function Curve-Json {ConvertTo-Json -InputObject ((Property 'rgb').value.value) -Compress -Depth 10}
    function Curve-At([double]$X,[double]$Y) {
        Show-Graph
        $hit=@{at=$null}
        Wait-Until {
            $r=(Control 'property-rgb-curve').Current.BoundingRectangle
            $at=@([int]($r.X+$X*$r.Width),[int]($r.Y+(1-$Y)*(Graph-Height)))
            $hit.at=$at
            !($at[0] -le $r.Left+6 -or $at[0] -ge $r.Right-6 -or $at[1] -le $r.Top+6 -or $at[1] -ge $r.Bottom-6)
        } 'Curve contact is outside the visible graph' 5
        $hit.at
    }
    function Move-Curve([string]$Device) {
        $point=(Property 'rgb').value.value[1]
        $from=Curve-At $point[0] $point[1];$to=Curve-At .7 .92
        [CapyRowPointer]::Down($Device,$from[0],$from[1])
        for($i=1;$i -le 12;$i++){
            [CapyRowPointer]::Move([int]($from[0]+($to[0]-$from[0])*$i/12),[int]($from[1]+($to[1]-$from[1])*$i/12))
            Start-Sleep -Milliseconds 35
        }
        Wait-Until {$p=(Property 'rgb').value.value[1];[Math]::Abs($p[0]-.7) -lt .01 -and [Math]::Abs($p[1]-.92) -lt .01} "$Device curve did not reach the drag target during contact"
    }
    function Check-Redo([string]$Reason) {
        Invoke 'Redo' -Name;Wait-Until {(Curve-Json) -eq $edited} "$Reason consumed Redo"
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$Reason changed history"
    }
    Pointer-Session {
        if((Field 'input').Current.IsEnabled -or (Field 'output').Current.IsEnabled){throw 'Input and Output were enabled without a selected point'}
        $unselected=(Property 'rgb').curve
        if(!$unselected -or $null -ne $unselected.selected -or $null -ne $unselected.input -or $null -ne $unselected.output){throw 'Unselected curve did not project null Input and Output'}
        foreach($axis in @('input','output')){
            if((Field $axis).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -cne ''){throw "Unselected curve $axis readout is not empty"}
        }
        $endpoints=Curve-Json
        # Seed a visible handle instead of resizing or scrolling the workspace.
        $at=Curve-At .5 .85
        [CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up()
        Wait-Until {(Property 'rgb').value.value.Count -eq 3} 'Pointer insertion did not add a curve point'
        $original=Curve-Json
        foreach($device in @('mouse','pen','touch')){
            Move-Curve $device;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            $edited=Curve-Json
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device drag needs more than one Undo"
            Check-Redo "$device completed drag"
            $point=(Property 'rgb').value.value[1];$at=Curve-At $point[0] $point[1]
            [CapyRowPointer]::Down($device,$at[0]+3,$at[1]+3);[CapyRowPointer]::Up()
            Start-Sleep -Milliseconds 150
            if((Curve-Json) -ne $original){throw "$device click moved the handle"}
            Check-Redo "$device unchanged click"
            $escaped=@{graph=$null}
            Move-Curve $device;[CapyRowPointer]::Key(0x1B)
            Wait-Until {
                $escaped.graph=Find 'property-rgb-curve' -Visible
                if((Curve-Json) -ne $original -or !$escaped.graph){return $false}
                $native=$escaped.graph.Current;$bounds=$native.BoundingRectangle
                $native.ProcessId-eq$review.Id -and $native.IsEnabled -and !$native.IsOffscreen -and !$bounds.IsEmpty -and $bounds.Width-gt0 -and $bounds.Height-gt0
            } "$device Escape did not restore the curve preview and visible native graph"
            [CapyRowPointer]::Up();Check-Redo "$device Escape"
            if($device -ne 'mouse'){
                Move-Curve $device;[CapyRowPointer]::Cancel()
                Wait-Until {(Curve-Json) -eq $original} "$device native cancellation kept the curve preview"
                Check-Redo "$device native cancellation"
            }
            Move-Curve $device;Select-Panel 'adjustments'
            Wait-Until {(Curve-Json) -eq $original} "$device hidden curve kept the preview"
            [CapyRowPointer]::Up();Select-Panel 'properties'
            Wait-Until {$null -ne (Find 'property-rgb-curve')} 'Curve did not reopen'
            Check-Redo "$device source hide"
            # A canceled insertion must also remove its provisional handle.
            $at=Curve-At .3 .9
            [CapyRowPointer]::Down($device,$at[0],$at[1])
            Wait-Until {(Property 'rgb').value.value.Count -eq 4} "$device insertion did not preview"
            [CapyRowPointer]::Key(0x1B)
            Wait-Until {(Curve-Json) -eq $original} "$device canceled insertion kept its handle"
            [CapyRowPointer]::Up();Check-Redo "$device canceled insertion"
            $at=Curve-At .3 .9;$to=Curve-At .36 .6
            [CapyRowPointer]::Down($device,$at[0],$at[1])
            Wait-Until {(Property 'rgb').value.value.Count -eq 4} "$device insertion did not preview"
            $inserted=(Property 'rgb').value.value[1]
            for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($at[0]+($to[0]-$at[0])*$i/8),[int]($at[1]+($to[1]-$at[1])*$i/8));Start-Sleep -Milliseconds 30}
            Wait-Until {$p=(Property 'rgb').value.value[1];[Math]::Abs($p[0]-$inserted[0]-.06) -lt .015 -and [Math]::Abs($p[1]-$inserted[1]+.3) -lt .015} "$device inserted point did not follow the same contact"
            [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            if((Property 'rgb').value.value.Count -ne 4){throw "$device insert and drag did not keep the point"}
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device insert and drag was not one Undo"
            Write-Host "$device curve: one-step history, unchanged click, Escape, source hide, insertion rollback and insert-drag passed"
        }
        if(!(Find 'property-rgb-reset')){throw 'Modified curve hides its reset icon'}
        foreach($device in @('mouse','pen','touch')){
            $point=(Property 'rgb').value.value[1];$at=Curve-At $point[0] $point[1]
            [CapyRowPointer]::DoubleClick($device,$at[0],$at[1])|Out-Null
            Wait-Until {(Property 'rgb').value.value.Count -eq 2} "$device double tap did not remove the point"
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device double tap removal was not one Undo"
            $point=(Property 'rgb').value.value[1];$from=Curve-At $point[0] $point[1]
            $r=(Control 'property-rgb-curve').Current.BoundingRectangle
            $away=@($from[0],[int]($r.Y-.25*(Graph-Height)))
            [CapyRowPointer]::Down($device,$from[0],$from[1])
            for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($from[0],[int]($from[1]+($away[1]-$from[1])*$i/10));Start-Sleep -Milliseconds 30}
            Wait-Until {(Property 'rgb').value.value.Count -eq 2} "$device drag beyond the graph did not remove the point"
            $back=Curve-At .5 .8
            for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($back[0],[int]($away[1]+($back[1]-$away[1])*$i/10));Start-Sleep -Milliseconds 30}
            Wait-Until {(Property 'rgb').value.value.Count -eq 3} "$device drag back did not restore the point"
            [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device detach and restore was not one Undo"
            Write-Host "$device curve: double tap removal and drag-off restore passed"
        }
        Tap-Point 1
        Wait-Until {(Field 'input').Current.IsEnabled -and (Field 'output').Current.IsEnabled} 'Selecting a point did not enable Input and Output'
        Wait-Until {(Field 'output').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq (Property 'rgb').curve.output.text} 'Output did not show the exact shared text'
        $revision=(Model).state.document_file.revision;$point=(Property 'rgb').value.value[1]
        Edit 'property-rgb-output' '1';Edit 'property-rgb-output' (Property 'rgb').curve.output.text;(Control 'property-rgb-curve').SetFocus();Start-Sleep -Milliseconds 300
        if((Curve-Json) -ne $original -or (Model).state.document_file.revision -ne $revision){throw 'Committing unchanged Output text changed the point'}
        Edit 'property-rgb-output' '200';(Control 'property-rgb-curve').SetFocus()
        Wait-Until {$p=(Property 'rgb').value.value;$p.Count -eq 3 -and [Math]::Abs($p[1][1]-200/255) -lt 1e-6 -and $p[1][0] -eq $point[0]} 'Typed Output did not move only the selected point'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'Typed Output was not one Undo'
        Tap-Point 1;$graph=Control 'property-rgb-curve';$graph.SetFocus();Wait-Until {$graph.Current.HasKeyboardFocus} 'The curve graph did not take focus'
        for($i=0;$i -lt 5;$i++){[CapyRowPointer]::Hold(0x26,$true);Start-Sleep -Milliseconds 30}
        [CapyRowPointer]::Hold(0x26,$false)
        try{Wait-Until {[Math]::Abs((Property 'rgb').value.value[1][1]-$point[1]-5/255) -lt 1e-4} 'A held Up arrow did not step the point by 1/255 per repeat'}
        catch{throw "Held Up: initial=$($point[1]), expected=$($point[1]+5/255), actual=$((Property 'rgb').value.value[1][1]), count=$((Property 'rgb').value.value.Count), selected=$((Property 'rgb').curve.selected), step=$((Property 'rgb').curve.numeric.step)"}
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'A held arrow was not one Undo'
        Tap-Point 1
        @{graph_focus=(Control 'property-rgb-curve').Current.HasKeyboardFocus;selected=(Property 'rgb').curve.selected;points=(Property 'rgb').value.value}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run 'curve-delete-focus.json')
        [CapyRowPointer]::Key(0x2E)
        Wait-Until {(Property 'rgb').value.value.Count -eq 2} 'Delete did not remove the selected point'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'Delete was not one Undo'
        Tap-Point 0
        Wait-Until {!(Field 'input').Current.IsEnabled -and (Field 'output').Current.IsEnabled} 'An endpoint Input was editable'
        $at=Curve-At .25 .6
        [CapyRowPointer]::DoubleClick('mouse',$at[0],$at[1])|Out-Null
        Start-Sleep -Milliseconds 400
        if((Property 'rgb').value.value.Count -ne 4){throw 'A double click on the empty graph did not insert exactly one point'}
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'The double-click insertion was not one Undo'
        if((Find 'property-hdr_stops') -or (Find 'property-domain')){throw 'An integer drawing offered the HDR curve domain'}
        Capture 'curve-gestures'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $endpoints} 'Seed insertion was not one Undo'
        'mouse, pen and touch: passed'
    }
}

try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme}|ConvertTo-Json))
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/effects-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned effects review $($review.Id)"
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window -and (Model).brush_ready} 'Review did not start' 45
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    Use-PaintWorkspace
    [CapyEffectsCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $resized=$false
    for($attempt=0;$attempt -lt 10 -and !$resized;$attempt++){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width 1550 -Height 1400
        try{Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Resize pending' 3;$resized=$true}catch{}
    }
    if(!$resized){throw 'Initial resize did not reach the canvas'}
    $root=(Owned-DrawingWindow $review $drawingWindow.ToInt64()).Root
    if($curveOnly){
        Select-Filter 'curves' 'Curves'
        $curveGestures=Check-CurveGestures
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;curve_pointer_transactions=$curveGestures;run=$run}|ConvertTo-Json
        return
    }
    if($PropertyLayout){
        Check-PropertyLayout
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;compact_opacity_both_themes='passed';typed_and_drag_one_undo='passed';filter_slider='passed';narrow_localized_caption_ellipsis='passed';retained_property_controls='passed';visual_review='required';run=$run}|ConvertTo-Json
        return
    }
    Select-Panel 'adjustments'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Curves preview not ready' 120
    $original=Preview-Hash 'curves';if(!$original){throw 'No initial preview pixels'}
    $rowIdentity=(Control 'filter-curves').GetRuntimeId() -join ':'
    Capture 'initial-previews'
    $revision=(Model).state.document_file.revision
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.revision -gt $revision -and ((Model).state.commands|Where-Object id -eq 'export_document').enabled} 'Stroke did not commit'
    if((Preview-Hash 'curves') -ne $original){throw 'Document edit changed the bundled preview'}
    if(((Control 'filter-curves').GetRuntimeId() -join ':') -ne $rowIdentity){throw 'Document edit replaced the filter row'}
    Capture 'paint-previews'
    Invoke 'Undo' -Name
    Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore checkpoint'
    Choose 'filter-category' 'Color'
    Wait-Until {(Model).state.filter_picker.category -eq 'color'} 'Category did not reach shared picker'
    Wait-Until {(Find 'filter-preview-hue_saturation').Current.ItemStatus -eq 'Ready'} 'New category preview not ready' 15
    Invoke 'filter-search-toggle';Edit 'filter-search' 'no-matching-filter-927'
    Wait-Until {(Model).state.adjustments.Count -eq 0} 'Unmatched search did not empty picker'
    if(Find 'filter-curves'){throw 'Search left an old insert button'}

    Select-Panel 'properties'
    Edit 'property-opacity' '60';(Control 'property-blend').SetFocus()
    Wait-Until {[Math]::Abs((Property 'opacity').value.value-.6) -lt .000001} 'Opacity not updated'
    Choose 'property-blend' 'Multiply';Wait-Until {(Property 'blend').value.value -eq 1} 'Blend not updated'
    Check-PropertyScrub
    Check-PropertyValueScrub
    Select-Filter 'curves' 'Curves'
    $curveGestures=Check-CurveGestures
    $graph=(Control 'property-rgb-curve').GetRuntimeId() -join ':'
    if(Find 'property-rgb-reset'){throw 'Unmodified curve shows its reset icon'}
    Invoke 'Redo' -Name;Wait-Until {(Property 'rgb').value.value.Count -eq 3} 'Curve point not restored'
    Wait-Until {$null -ne (Find 'property-rgb-reset')} 'Modified curve hides its reset icon'
    if(((Control 'property-rgb-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Editing replaced the curve graph'}
    Capture 'curve'
    $wide=(Model).state.camera.viewport[0]
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width 1450 -Height 1000
    Wait-Until {(Model).state.camera.viewport[0] -lt $wide-50} 'Resize did not reach the canvas'
    if(((Control 'property-rgb-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Resize replaced the curve graph'}
    Invoke 'property-rgb-reset'
    Wait-Until {(Property 'rgb').value.value.Count -eq 2} 'Curve reset failed'
    if((Property 'rgb').value.value[0][1] -ne 0 -or (Property 'rgb').value.value[1][1] -ne 1){throw 'Reset curve kept an edited endpoint'}
    Wait-Until {!(Find 'property-rgb-reset')} 'Reset curve still shows its reset icon'
    $revision=(Model).state.document_file.revision;$pages=@((Model).state.layer_properties.pages)
    if($pages.Count -ne 4){throw 'Curves did not publish its four pages'}
    Choose 'properties-page' $pages[1].label
    Wait-Until {(Model).state.layer_properties.page -eq $pages[1].id -and (Find 'property-red-curve') -and !(Find 'property-rgb-curve')} 'The Properties page did not show the second curve'
    Choose 'properties-page' $pages[0].label
    Wait-Until {(Model).state.layer_properties.page -eq $pages[0].id -and (Find 'property-rgb-curve')} 'The Properties page did not return to the composite curve'
    if((Model).state.document_file.revision -ne $revision){throw 'Changing the Properties page added an undo step'}
    Select-Filter 'color_balance' 'Color Balance'
    foreach($page in @((Model).state.layer_properties.pages)){
        Choose 'properties-page' $page.label
        Wait-Until {$keys=@((Model).state.layer_properties.controls|Where-Object page -eq $page.id|ForEach-Object key);(Model).state.layer_properties.page -eq $page.id -and $keys.Count -and @($keys|Where-Object {!(Find "property-$_")}).Count -eq 0} "Color Balance did not present its $($page.label) page"
    }
    Select-Filter 'brightness_contrast' 'Brightness / Contrast'
    if(Find 'properties-page'){throw 'A single-page filter offered a page choice'}
    Select-Filter 'hue_saturation' 'Hue / Saturation'
    Wait-Until {@((Model).state.layer_properties.pages).Count -eq 7 -and (Find 'properties-page')} 'Hue / Saturation did not offer Master and six ranges'
    $lightness=Control 'property-lightness';$identity=$lightness.GetRuntimeId() -join ':'
    $lightness.SetFocus();$lightness.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('12')
    (Control 'property-colorize').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Property 'colorize').value.value -and (Find 'property-colorize_hue') -and !(Find 'property-hue') -and !(Find 'properties-page')} 'Colorize did not replace Hue and Saturation and hide the ranges'
    if(((Control 'property-lightness').GetRuntimeId() -join ':') -ne $identity){throw 'Colorize replaced the common Lightness field'}
    Wait-Until {[Math]::Abs((Property 'lightness').value.value-12) -lt 1e-6} 'The Lightness draft did not commit when Colorize took focus'
    Capture 'hue-colorize'
    Select-Filter 'photo_filter' 'Photo Filter'
    foreach($id in @('property-color-color','property-density','property-preserve_luminance')){$null=Control $id}
    Select-Filter 'threshold' 'Threshold';$null=Control 'property-threshold'
    foreach($filter in @(@('invert','Invert'),@('desaturate','Desaturate'))){Select-Filter $filter[0] $filter[1];if(@((Model).state.layer_properties.controls).Count){throw "$($filter[1]) offered controls"}}
    Select-Filter 'selective_color' 'Selective Color'
    $pages=@((Model).state.layer_properties.pages)
    if($pages.Count -ne 9){throw 'Selective Color did not offer nine color pages'}
    Choose 'properties-page' $pages[7].label
    Wait-Until {(Model).state.layer_properties.page -eq 'neutrals' -and (Find 'property-neutrals_cyan') -and !(Find 'property-reds_cyan') -and (Find 'property-mode')} 'Selective Color did not present Neutrals beside the shared method'
    Edit 'property-neutrals_cyan' '-20';(Control 'property-neutrals_magenta').SetFocus()
    Wait-Until {[Math]::Abs((Property 'neutrals_cyan').value.value+20) -lt 1e-6} 'Neutrals Cyan did not commit'
    Choose 'property-mode' (Property 'mode').kind.options[1]
    Wait-Until {(Property 'mode').value.value -eq 1 -and [Math]::Abs((Property 'neutrals_cyan').value.value+20) -lt 1e-6} 'Selective Color did not switch to Absolute and keep Neutrals'
    Capture 'selective-color'
    Select-Filter 'channel_mixer' 'Channel Mixer'
    Wait-Until {(@((Model).state.layer_properties.pages)|ForEach-Object id) -join ',' -eq 'red,green,blue'} 'Channel Mixer did not offer red, green and blue rows'
    $monochrome=Control 'property-monochrome';$identity=$monochrome.GetRuntimeId() -join ':'
    $monochrome.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Property 'monochrome').value.value -and (Find 'property-gray_red') -and !(Find 'property-red_red') -and !(Find 'properties-page')} 'Monochrome did not replace the color rows with the gray row'
    if(((Control 'property-monochrome').GetRuntimeId() -join ':') -ne $identity){throw 'Monochrome replaced its own toggle'}
    Capture 'channel-mixer'

    Select-Filter 'gradient_map' 'Gradient Map'
    Check-Gradient
    Edit 'property-gradient-position' '35';(Control 'property-gradient-color').SetFocus()
    Wait-Until {[Math]::Abs((Property 'gradient').value.value.stops[1].position-.35) -lt .000001} 'Gradient position not updated'
    $alpha=(Rgba (Property 'gradient').value.value.stops[1].color)[3]
    Invoke 'property-gradient-color';Edit-Value 'edit-color-hex' '#336699';Invoke 'edit-color-apply'
    Wait-Until {$stop=Rgba (Property 'gradient').value.value.stops[1].color;[Math]::Abs($stop[2]-.6) -lt .002 -and [Math]::Abs($stop[0]-.2) -lt .002 -and $stop[3] -eq $alpha} 'Gradient stop color not updated'
    Wait-Until {!(Find 'edit-color-apply')} 'Use Color did not close Edit Color'
    Capture 'gradient'
    Invoke 'property-gradient-color';Edit-Value 'edit-color-0-0' '90';Invoke 'edit-color-cancel'
    Wait-Until {!(Find 'edit-color-apply')} 'Cancel did not close Edit Color'
    Invoke 'property-gradient-reset'
    Wait-Until {(Property 'gradient').value.value.stops.Count -eq 2} 'Gradient reset failed'
    if((Rgba (Property 'gradient').value.value.stops[0].color)[0] -ne 0){throw 'A cancelled draft leaked into the reset gradient'}
    if((Control 'property-gradient-position').Current.IsEnabled){throw 'Gradient endpoint position should be disabled'}
    Select-Filter 'split_tone' 'Split Tone';Invoke 'property-shadows-color'
    Edit-Form 0 'rgb_unit';Edit-Value 'edit-color-0-0' '0.25';Invoke 'edit-color-apply'
    Wait-Until {[Math]::Abs((Rgba (Property 'shadows').value.value)[0]-.25) -lt .000001} 'Scalar color not updated'
    if([Math]::Abs((Rgba (Property 'shadows').value.value)[1]-.33) -gt .000001){throw 'Scalar channel edit changed another channel'}
    Capture 'color'
    $theme=(Model).state.theme
    Open-Preferences
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=if($theme -eq 'dark'){'Light'}else{'Dark'}
    (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
    Invoke 'CloseButton'
    Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
    Capture 'alternate-theme'
    Select-Panel 'adjustments'
    Wait-Until {@((Model).layout.groups|Where-Object {$_.active -eq 'adjustments'}).Count -gt 0} 'Filters tab did not reopen'
    Edit 'filter-search' 'C';Edit 'filter-search' 'Cur';Edit 'filter-search' 'Curves'
    Wait-Until {(Model).state.filter_picker.search -eq 'Curves'} 'Rapid search edits were lost'
    Wait-Until {(Control 'filter-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'Curves'} 'Search text differs from its acknowledged query'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after filter edits and theme not ready' 20
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'new_document'
    New-DrawingPixels 128 64
    $depth=Control 'document-depth';$depth.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $float=@{item=$null};Wait-Until {$float.item=@($depth.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))|Where-Object {$_.Current.Name -match 'float'})[0];$float.item} 'New drawing did not offer a float depth'
    $float.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select();Invoke 'Create' -Name
    Wait-Until {$active=@((Model).state.tabs|Where-Object active);$active.Count -eq 1 -and $active[0].width -eq 128 -and $active[0].height -eq 64 -and !(Model).state.document_file.busy} 'Document replacement failed' 45
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Document dialog gate did not clear'
    if($null -eq (Model).state.filter_picker.search){Invoke 'filter-search-toggle'}
    Edit 'filter-search' 'Curves'
    Wait-Until {(Model).state.filter_picker.search -eq 'Curves'} 'Search did not reach the new drawing'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after document replacement not ready' 20
    Select-Panel 'properties'
    Wait-Until {(Property 'opacity').value.value -eq 1 -and (Property 'blend').value.value -eq 0} 'Replacement reused old property values'
    if((Model).state.document_file.modified){throw 'Preview or stale property changed new document'}
    Select-Panel 'adjustments'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Retained cache not shown on reopen' 15
    Check-LogCurve
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [PSCustomObject]@{
        curve_pointer_transactions=$curveGestures
        six_property_kinds='passed';reset_draft_guards_and_endpoints='passed'
        curve_control_retention_and_resize='passed';category_search_and_insertion='passed'
        gpu_preview_paint_and_exact_undo='passed';preview_theme_and_document_replacement='passed'
        properties_pages='passed';curve_fields_keys_and_log_hdr='passed';one_undo_slider_scrub='passed'
        clean_document_and_zero_exit='passed'
        scope='isolated native UI and app-only GPU pixels; full workspace visual parity, physical input and presentation acceptance remain separate'
    }|ConvertTo-Json
}catch{
    try{Capture 'failure' -WithModel}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
