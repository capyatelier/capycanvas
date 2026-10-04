param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true;$CapyCaptureDelay=250
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
function Invoke([string]$Value,[switch]$Name){
    $hit=@{item=$null}
    Wait-Until {
        $hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::Button)
        if(!$hit.item){$hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $null -ne $hit.item
    } "Missing action: $Value"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Edit([string]$Id,[string]$Text){
    $entry=Control $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object {$_.key -eq $Key}}
function Rgba($Color){if($null -ne $Color.rgba){$Color.rgba}else{$Color}}
function Choose([string]$Id,[string]$Option){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Select-Panel([string]$Id){
    # Repeating an active tab opens panel configuration. Insertion can select
    # Properties itself, so select only when the shared active panel differs.
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
    if(![CapyEffectsCapture]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($window.right-$window.left,$window.bottom-$window.top)
    try{
        $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
        try{if(![CapyEffectsCapture]::PrintWindow($review.MainWindowHandle,$dc,2)){throw 'App capture failed'}}
        finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
        $area=[Drawing.Rectangle]::new([int]$r.X-$window.left+2,[int]$r.Y-$window.top+2,[int]$r.Width-4,[int]$r.Height-4)
        $crop=$bitmap.Clone($area,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stream=[IO.MemoryStream]::new();$sha=[Security.Cryptography.SHA256]::Create()
        try{$crop.Save($stream,[Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($sha.ComputeHash($stream.ToArray()))}
        finally{$crop.Dispose();$stream.Dispose();$sha.Dispose()}
    }finally{$bitmap.Dispose()}
}

function Field([string]$Axis){Control "property-rgb-$Axis"}
function Show-Graph([double]$Percent=0){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent((Control 'property-rgb-curve'))
    while($node){$scroll=$null;if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable){break};$node=$walker.GetParent($node)}
    if($node -and $scroll.Current.VerticalScrollPercent -ne $Percent){$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,$Percent)}
    $settled=@{bounds=$null};Wait-Until {$bounds=(Control 'property-rgb-curve').Current.BoundingRectangle;$same=$bounds -eq $settled.bounds;$settled.bounds=$bounds;Start-Sleep -Milliseconds 100;$same} 'The curve graph did not settle after scrolling'
}
function Tap-Point([int]$Index){
    $point=(Property 'rgb').value.value[$Index];$low=$point[1] -lt .5;Show-Graph $(if($low){100}else{0})
    $r=(Control 'property-rgb-curve').Current.BoundingRectangle;$top=if($low){$r.Bottom-$r.Width}else{$r.Y}
    $at=@([int][Math]::Min([Math]::Max($r.X+$point[0]*$r.Width,$r.Left+10),$r.Right-10),[int][Math]::Min([Math]::Max($top+(1-$point[1])*$r.Width,$r.Top+10),$r.Bottom-10))
    [CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up()
    Wait-Until {(Property 'rgb').curve.selected -eq $Index} "Tapping point $Index did not select it"
}
function Pointer-Session([scriptblock]$Body){
    Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    try{& $Body}finally{[CapyRowPointer]::Dispose()}
}
function Stops{@((Property 'gradient').value.value.stops)}
function Gradient-Json{(Property 'gradient').value.value|ConvertTo-Json -Depth 10 -Compress}
function Near-Stop([double]$Position){@(Stops|Where-Object {[Math]::Abs($_.position-$Position) -lt .02}).Count -gt 0}
function Strip-Point([double]$Fraction){
    $r=(Control 'property-gradient-gradient').Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
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
    [CapyRowPointer]::Up();Wait-Until {[Math]::Abs((Property 'opacity').value.value-.7) -lt .03} 'The opacity slider did not follow the drag'
    Invoke 'Undo' -Name;Wait-Until {(Property 'opacity').value.value -eq $opacity} 'An opacity slider drag needed more than one Undo'
}}
function Check-LogCurve {Pointer-Session {
    $grown=$false
    for($attempt=0;$attempt -lt 5 -and !$grown;$attempt++){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1550 -Height 1400
        try{Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Resize pending' 3;$grown=$true}catch{}
    }
    if(!$grown){throw 'The window did not grow for the curve check'}
    Select-Filter 'curves' 'Curves'
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
        # The graph is square; UIA can report only its visible, clipped height.
        $hit=@{at=$null}
        Wait-Until {
            $r=(Control 'property-rgb-curve').Current.BoundingRectangle
            $at=@([int]($r.X+$X*$r.Width),[int]($r.Y+(1-$Y)*$r.Width))
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
            Move-Curve $device;[CapyRowPointer]::Key(0x1B)
            Wait-Until {(Curve-Json) -eq $original} "$device Escape kept the curve preview"
            if(!(Find 'property-rgb-curve')){throw "$device Escape dismissed the curve drawer"}
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
            for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($at[0]+($to[0]-$at[0])*$i/8),[int]($at[1]+($to[1]-$at[1])*$i/8));Start-Sleep -Milliseconds 30}
            Wait-Until {$p=(Property 'rgb').value.value[1];[Math]::Abs($p[0]-.36) -lt .015 -and [Math]::Abs($p[1]-.6) -lt .015} "$device inserted point did not follow the same contact"
            [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            if((Property 'rgb').value.value.Count -ne 4){throw "$device insert and drag did not keep the point"}
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device insert and drag was not one Undo"
            Write-Host "$device curve: one-step history, unchanged click, Escape, source hide, insertion rollback and insert-drag passed"
        }
        if(!(Find 'property-rgb-reset')){throw 'Modified curve hides its reset icon'}
        foreach($device in @('mouse','pen','touch')){
            $point=(Property 'rgb').value.value[1];$at=Curve-At $point[0] $point[1]
            [CapyRowPointer]::Down($device,$at[0],$at[1]);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 60
            [CapyRowPointer]::Down($device,$at[0],$at[1]);[CapyRowPointer]::Up()
            Wait-Until {(Property 'rgb').value.value.Count -eq 2} "$device double tap did not remove the point"
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device double tap removal was not one Undo"
            $point=(Property 'rgb').value.value[1];$from=Curve-At $point[0] $point[1]
            $r=(Control 'property-rgb-curve').Current.BoundingRectangle
            $away=@($from[0],[int]($r.Y-.25*$r.Width))
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
        for($i=0;$i -lt 5;$i++){[CapyRowPointer]::Hold(0x26,$true);Start-Sleep -Milliseconds 60}
        [CapyRowPointer]::Hold(0x26,$false)
        Wait-Until {[Math]::Abs((Property 'rgb').value.value[1][1]-$point[1]-5/255) -lt 1e-4} 'A held Up arrow did not step the point by 1/255 per repeat'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'A held arrow was not one Undo'
        Tap-Point 1;[CapyRowPointer]::Key(0x2E)
        Wait-Until {(Property 'rgb').value.value.Count -eq 2} 'Delete did not remove the selected point'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} 'Delete was not one Undo'
        Tap-Point 0
        Wait-Until {!(Field 'input').Current.IsEnabled -and (Field 'output').Current.IsEnabled} 'An endpoint Input was editable'
        $at=Curve-At .25 .6
        for($i=0;$i -lt 2;$i++){[CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 60}
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
    if(Get-Process CapyCanvas -ErrorAction SilentlyContinue){throw 'Close the existing app before the isolated effects review'}
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/effects-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned effects review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyEffectsCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $resized=$false
    for($attempt=0;$attempt -lt 10 -and !$resized;$attempt++){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1550 -Height 1400
        try{Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Resize pending' 3;$resized=$true}catch{}
    }
    if(!$resized){throw 'Initial resize did not reach the canvas'}
    $root=$null
    Wait-Until {try{$script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)}catch{};$null -ne $root} 'Resized window has no automation root' 20
    Select-Panel 'adjustments'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Curves preview not ready' 120
    $original=Preview-Hash 'curves';if(!$original){throw 'No initial preview pixels'}
    $rowIdentity=(Control 'filter-curves').GetRuntimeId() -join ':'
    Capture 'initial-previews'
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Stroke not acknowledged'
    Wait-Until {$changed=Preview-Hash 'curves';$changed -and $changed -ne $original} 'Preview did not sample edited paint' 15
    if(((Control 'filter-curves').GetRuntimeId() -join ':') -ne $rowIdentity){throw 'Document edit replaced the filter row'}
    Capture 'paint-previews'
    Invoke 'Undo' -Name
    Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore checkpoint'
    Wait-Until {(Preview-Hash 'curves') -eq $original} 'Undo did not restore preview pixels' 15
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
    Select-Filter 'curves' 'Curves'
    $curveGestures=Check-CurveGestures
    $graph=(Control 'property-rgb-curve').GetRuntimeId() -join ':'
    if(Find 'property-rgb-reset'){throw 'Unmodified curve shows its reset icon'}
    Invoke 'Redo' -Name;Wait-Until {(Property 'rgb').value.value.Count -eq 3} 'Curve point not restored'
    Wait-Until {$null -ne (Find 'property-rgb-reset')} 'Modified curve hides its reset icon'
    if(((Control 'property-rgb-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Editing replaced the curve graph'}
    Capture 'curve'
    $wide=(Model).state.camera.viewport[0]
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1450 -Height 1000
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
    Invoke 'property-gradient-color';Edit 'property-gradient-color-3' '40';Invoke 'property-gradient-color-apply'
    Wait-Until {[Math]::Abs((Rgba (Property 'gradient').value.value.stops[1].color)[3]-.4) -lt .000001} 'Gradient alpha not updated'
    Capture 'gradient'
    Edit 'property-gradient-color-0' '90';Invoke 'property-gradient-reset'
    Wait-Until {(Property 'gradient').value.value.stops.Count -eq 2} 'Gradient reset failed'
    if((Rgba (Property 'gradient').value.value.stops[0].color)[0] -ne 0){throw 'Draft leaked into reset gradient'}
    if((Control 'property-gradient-position').Current.IsEnabled){throw 'Gradient endpoint position should be disabled'}
    Select-Filter 'split_tone' 'Split Tone';Invoke 'property-shadows-color'
    Edit 'property-shadows-color-0' '0.25';Invoke 'property-shadows-color-apply'
    Wait-Until {[Math]::Abs((Rgba (Property 'shadows').value.value)[0]-.25) -lt .000001} 'Scalar color not updated'
    if([Math]::Abs((Rgba (Property 'shadows').value.value)[1]-.33) -gt .000001){throw 'Scalar channel edit changed another channel'}
    Capture 'color'
    $theme=(Model).state.theme
    Invoke 'settings-button'
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
    $discard=@{item=$null};try{Wait-Until {$discard.item=Find 'Discard Changes' -Name;$null -ne $discard.item -or $null -ne (Find 'document-width')} 'New drawing did not open' 5}catch{}
    if($discard.item){$discard.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    Edit 'document-width' '128';Edit 'document-height' '64'
    $depth=Control 'document-depth';$depth.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $float=@{item=$null};Wait-Until {$float.item=@($depth.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))|Where-Object {$_.Current.Name -match 'float'})[0];$float.item} 'New drawing did not offer a float depth'
    $float.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select();Invoke 'Create' -Name
    Wait-Until {$active=@((Model).state.tabs|Where-Object active);$active.Count -eq 1 -and $active[0].width -eq 128 -and $active[0].height -eq 64 -and !(Model).state.document_file.busy} 'Document replacement failed' 45
    Wait-Until {(Control 'drawing-canvas').Current.IsEnabled} 'Document dialog gate did not clear'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after document replacement not ready' 20
    Select-Panel 'properties'
    Wait-Until {(Property 'opacity').value.value -eq 1 -and (Property 'blend').value.value -eq 0} 'Replacement reused old property values'
    if((Model).state.document_file.modified){throw 'Preview or stale property changed new document'}
    Select-Panel 'adjustments'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Retained cache not shown on reopen' 15
    Check-LogCurve
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
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
    try{Capture 'failure'}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
