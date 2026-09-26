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

function Check-CurveGestures {
    Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    function Curve-Json {ConvertTo-Json -InputObject ((Property 'curve_0').value.value) -Compress -Depth 10}
    function Curve-At([double]$X,[double]$Y) {
        # The graph is square; UIA can report only its visible, clipped height.
        $hit=@{at=$null}
        Wait-Until {
            $r=(Control 'property-curve_0-curve').Current.BoundingRectangle
            $at=@([int]($r.X+$X*$r.Width),[int]($r.Y+(1-$Y)*$r.Width))
            $hit.at=$at
            !($at[0] -le $r.Left+6 -or $at[0] -ge $r.Right-6 -or $at[1] -le $r.Top+6 -or $at[1] -ge $r.Bottom-6)
        } 'Curve contact is outside the visible graph' 5
        $hit.at
    }
    function Move-Curve([string]$Device) {
        $point=(Property 'curve_0').value.value[1]
        $from=Curve-At $point[0] $point[1];$to=Curve-At .7 .92
        [CapyRowPointer]::Down($Device,$from[0],$from[1])
        for($i=1;$i -le 12;$i++){
            [CapyRowPointer]::Move([int]($from[0]+($to[0]-$from[0])*$i/12),[int]($from[1]+($to[1]-$from[1])*$i/12))
            Start-Sleep -Milliseconds 35
        }
        Wait-Until {$p=(Property 'curve_0').value.value[1];[Math]::Abs($p[0]-.7) -lt .01 -and [Math]::Abs($p[1]-.92) -lt .01} "$Device curve did not reach the drag target during contact"
    }
    function Check-Redo([string]$Reason) {
        Invoke 'Redo' -Name;Wait-Until {(Curve-Json) -eq $edited} "$Reason consumed Redo"
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$Reason changed history"
    }
    try {
        $endpoints=Curve-Json
        # Seed a visible handle instead of resizing or scrolling the workspace.
        $at=Curve-At .5 .85
        [CapyRowPointer]::Down('mouse',$at[0],$at[1]);[CapyRowPointer]::Up()
        Wait-Until {(Property 'curve_0').value.value.Count -eq 3} 'Pointer insertion did not add a curve point'
        $original=Curve-Json
        foreach($device in @('mouse','pen','touch')){
            Move-Curve $device;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            $edited=Curve-Json
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device drag needs more than one Undo"
            Check-Redo "$device completed drag"
            $point=(Property 'curve_0').value.value[1];$at=Curve-At $point[0] $point[1]
            [CapyRowPointer]::Down($device,$at[0]+3,$at[1]+3);[CapyRowPointer]::Up()
            Start-Sleep -Milliseconds 150
            if((Curve-Json) -ne $original){throw "$device click moved the handle"}
            Check-Redo "$device unchanged click"
            Move-Curve $device;[CapyRowPointer]::Key(0x1B)
            Wait-Until {(Curve-Json) -eq $original} "$device Escape kept the curve preview"
            if(!(Find 'property-curve_0-curve')){throw "$device Escape dismissed the curve drawer"}
            [CapyRowPointer]::Up();Check-Redo "$device Escape"
            if($device -ne 'mouse'){
                Move-Curve $device;[CapyRowPointer]::Cancel()
                Wait-Until {(Curve-Json) -eq $original} "$device native cancellation kept the curve preview"
                Check-Redo "$device native cancellation"
            }
            Move-Curve $device;Select-Panel 'adjustments'
            Wait-Until {(Curve-Json) -eq $original} "$device hidden curve kept the preview"
            [CapyRowPointer]::Up();Select-Panel 'properties'
            Wait-Until {$null -ne (Find 'property-curve_0-curve')} 'Curve did not reopen'
            Check-Redo "$device source hide"
            # A canceled insertion must also remove its provisional handle.
            $at=Curve-At .3 .9
            [CapyRowPointer]::Down($device,$at[0],$at[1])
            Wait-Until {(Property 'curve_0').value.value.Count -eq 4} "$device insertion did not preview"
            [CapyRowPointer]::Key(0x1B)
            Wait-Until {(Curve-Json) -eq $original} "$device canceled insertion kept its handle"
            [CapyRowPointer]::Up();Check-Redo "$device canceled insertion"
            $at=Curve-At .3 .9;$to=Curve-At .36 .6
            [CapyRowPointer]::Down($device,$at[0],$at[1])
            Wait-Until {(Property 'curve_0').value.value.Count -eq 4} "$device insertion did not preview"
            for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move([int]($at[0]+($to[0]-$at[0])*$i/8),[int]($at[1]+($to[1]-$at[1])*$i/8));Start-Sleep -Milliseconds 30}
            Wait-Until {$p=(Property 'curve_0').value.value[1];[Math]::Abs($p[0]-.36) -lt .015 -and [Math]::Abs($p[1]-.6) -lt .015} "$device inserted point did not follow the same contact"
            [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            if((Property 'curve_0').value.value.Count -ne 4){throw "$device insert and drag did not keep the point"}
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device insert and drag was not one Undo"
            Write-Host "$device curve: one-step history, unchanged click, Escape, source hide, insertion rollback and insert-drag passed"
        }
        if(!(Find 'property-curve_0-reset')){throw 'Modified curve hides its reset icon'}
        foreach($device in @('mouse','pen','touch')){
            $point=(Property 'curve_0').value.value[1];$at=Curve-At $point[0] $point[1]
            [CapyRowPointer]::Down($device,$at[0],$at[1]);[CapyRowPointer]::Up();Start-Sleep -Milliseconds 60
            [CapyRowPointer]::Down($device,$at[0],$at[1]);[CapyRowPointer]::Up()
            Wait-Until {(Property 'curve_0').value.value.Count -eq 2} "$device double tap did not remove the point"
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device double tap removal was not one Undo"
            $point=(Property 'curve_0').value.value[1];$from=Curve-At $point[0] $point[1]
            $r=(Control 'property-curve_0-curve').Current.BoundingRectangle
            $away=@($from[0],[int]($r.Y-.25*$r.Width))
            [CapyRowPointer]::Down($device,$from[0],$from[1])
            for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($from[0],[int]($from[1]+($away[1]-$from[1])*$i/10));Start-Sleep -Milliseconds 30}
            Wait-Until {(Property 'curve_0').value.value.Count -eq 2} "$device drag beyond the graph did not remove the point"
            $back=Curve-At .5 .8
            for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($back[0],[int]($away[1]+($back[1]-$away[1])*$i/10));Start-Sleep -Milliseconds 30}
            Wait-Until {(Property 'curve_0').value.value.Count -eq 3} "$device drag back did not restore the point"
            [CapyRowPointer]::Up();Start-Sleep -Milliseconds 150
            Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $original} "$device detach and restore was not one Undo"
            Write-Host "$device curve: double tap removal and drag-off restore passed"
        }
        Capture 'curve-gestures'
        Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $endpoints} 'Seed insertion was not one Undo'
        'mouse, pen and touch: passed'
    } finally {[CapyRowPointer]::Dispose()}
}

try {
    if(Get-Process CapyCanvas -ErrorAction SilentlyContinue){throw 'Close the existing app before the isolated effects review'}
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
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
    Select-Filter 'curves' 'Curves'
    $curveGestures=Check-CurveGestures
    $graph=(Control 'property-curve_0-curve').GetRuntimeId() -join ':'
    if(Find 'property-curve_0-reset'){throw 'Unmodified curve shows its reset icon'}
    Invoke 'Redo' -Name;Wait-Until {(Property 'curve_0').value.value.Count -eq 3} 'Curve point not restored'
    Wait-Until {$null -ne (Find 'property-curve_0-reset')} 'Modified curve hides its reset icon'
    if(((Control 'property-curve_0-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Editing replaced the curve graph'}
    Capture 'curve'
    $wide=(Model).state.camera.viewport[0]
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1450 -Height 1000
    Wait-Until {(Model).state.camera.viewport[0] -lt $wide-50} 'Resize did not reach the canvas'
    if(((Control 'property-curve_0-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Resize replaced the curve graph'}
    Invoke 'property-curve_0-reset'
    Wait-Until {(Property 'curve_0').value.value.Count -eq 2} 'Curve reset failed'
    if((Property 'curve_0').value.value[0][1] -ne 0 -or (Property 'curve_0').value.value[1][1] -ne 1){throw 'Reset curve kept an edited endpoint'}
    Wait-Until {!(Find 'property-curve_0-reset')} 'Reset curve still shows its reset icon'

    Select-Filter 'gradient_map' 'Gradient Map'
    Invoke 'property-gradient-add';Wait-Until {(Property 'gradient').value.value.Count -eq 3} 'Gradient stop not added'
    Edit 'property-gradient-position' '35';(Control 'property-gradient-color').SetFocus()
    Wait-Until {[Math]::Abs((Property 'gradient').value.value[1].position-.35) -lt .000001} 'Gradient position not updated'
    Invoke 'property-gradient-color';Edit 'property-gradient-color-3' '40';Invoke 'property-gradient-color-apply'
    Wait-Until {[Math]::Abs((Rgba (Property 'gradient').value.value[1].color)[3]-.4) -lt .000001} 'Gradient alpha not updated'
    if([Math]::Abs((Rgba (Property 'gradient').value.value[1].color)[0]-.5) -gt .000001){throw 'Alpha edit changed RGB'}
    (Control 'property-reverse').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Property 'reverse').value.value} 'Reverse toggle not updated'
    Capture 'gradient'
    Edit 'property-gradient-color-0' '90';Invoke 'property-gradient-reset'
    Wait-Until {(Property 'gradient').value.value.Count -eq 2} 'Gradient reset failed'
    if((Rgba (Property 'gradient').value.value[0].color)[0] -ne 0){throw 'Draft leaked into reset gradient'}
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
    Edit 'document-width' '128';Edit 'document-height' '64';Invoke 'Create' -Name
    Wait-Until {$active=@((Model).state.tabs|Where-Object active);$active.Count -eq 1 -and $active[0].width -eq 128 -and $active[0].height -eq 64 -and !(Model).state.document_file.busy} 'Document replacement failed' 45
    Wait-Until {(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Document dialog gate did not clear'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after document replacement not ready' 20
    Select-Panel 'properties'
    Wait-Until {(Property 'opacity').value.value -eq 1 -and (Property 'blend').value.value -eq 0} 'Replacement reused old property values'
    if((Model).state.document_file.modified){throw 'Preview or stale property changed new document'}
    Select-Panel 'adjustments'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Retained cache not shown on reopen' 15
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [PSCustomObject]@{
        curve_pointer_transactions=$curveGestures
        six_property_kinds='passed';reset_draft_guards_and_endpoints='passed'
        curve_control_retention_and_resize='passed';category_search_and_insertion='passed'
        gpu_preview_paint_and_exact_undo='passed';preview_theme_and_document_replacement='passed'
        clean_document_and_zero_exit='passed'
        scope='isolated native UI and app-only GPU pixels; full workspace visual parity, physical input and presentation acceptance remain separate'
    }|ConvertTo-Json
}catch{
    try{Capture 'failure'}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
