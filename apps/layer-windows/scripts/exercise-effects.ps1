param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
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
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}

function Model {
    try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Effects review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $match=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$match=[System.Windows.Automation.AndCondition]::new($match,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$match)
}
function Control([string]$Value,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
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
function Edit([string]$Id,[string]$Text){
    $entry=Control $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Property([string]$Key){(Model).state.layer_properties.controls|Where-Object {$_.key -eq $Key}}
function Choose([string]$Id,[string]$Option){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Select-Filter([string]$Id,[string]$Label){
    Invoke 'Filters' -Name
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
    Invoke 'Properties' -Name
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
function Capture([string]$Name){
    Start-Sleep -Milliseconds 250 # Allow acknowledged layout/theme changes to reach composition.
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}

try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
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
    Invoke 'Filters' -Name
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Curves preview not ready' 20
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

    Invoke 'Properties' -Name
    Edit 'property-opacity' '60';(Control 'property-blend').SetFocus()
    Wait-Until {[Math]::Abs((Property 'opacity').value.value-.6) -lt .000001} 'Opacity not updated'
    Choose 'property-blend' 'Multiply';Wait-Until {(Property 'blend').value.value -eq 1} 'Blend not updated'
    Select-Filter 'curves' 'Curves'
    $graph=(Control 'property-curve_0-curve').GetRuntimeId() -join ':'
    Invoke 'property-curve_0-add';Wait-Until {(Property 'curve_0').value.value.Count -eq 3} 'Curve point not added'
    Invoke 'property-curve_0-values';Edit 'property-curve_0-y' '75';(Control 'property-curve_0-point').SetFocus()
    Wait-Until {[Math]::Abs((Property 'curve_0').value.value[1][1]-.75) -lt .000001} 'Curve output not updated'
    if(((Control 'property-curve_0-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Editing replaced the curve graph'}
    Capture 'curve'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1550 -Height 1040
    Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Resize did not reach the canvas'
    if(((Control 'property-curve_0-curve').GetRuntimeId() -join ':') -ne $graph){throw 'Resize replaced the curve graph'}
    Edit 'property-curve_0-y' '90';Invoke 'property-curve_0-reset'
    Wait-Until {(Property 'curve_0').value.value.Count -eq 2} 'Curve reset failed'
    if((Property 'curve_0').value.value[0][1] -ne 0 -or (Property 'curve_0').value.value[1][1] -ne 1){throw 'Draft leaked into reset curve'}
    if((Control 'property-curve_0-x').Current.IsEnabled){throw 'Curve endpoint X should be disabled'}

    Select-Filter 'gradient_map' 'Gradient Map'
    Invoke 'property-gradient-add';Wait-Until {(Property 'gradient').value.value.Count -eq 3} 'Gradient stop not added'
    Edit 'property-gradient-position' '35';(Control 'property-gradient-color').SetFocus()
    Wait-Until {[Math]::Abs((Property 'gradient').value.value[1].position-.35) -lt .000001} 'Gradient position not updated'
    Invoke 'property-gradient-color';Edit 'property-gradient-color-3' '40';(Control 'property-gradient-color-0').SetFocus()
    Wait-Until {[Math]::Abs((Property 'gradient').value.value[1].color[3]-.4) -lt .000001} 'Gradient alpha not updated'
    if([Math]::Abs((Property 'gradient').value.value[1].color[0]-.5) -gt .000001){throw 'Alpha edit changed RGB'}
    (Control 'property-reverse').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Property 'reverse').value.value} 'Reverse toggle not updated'
    Capture 'gradient'
    Edit 'property-gradient-color-0' '90';Invoke 'property-gradient-reset'
    Wait-Until {(Property 'gradient').value.value.Count -eq 2} 'Gradient reset failed'
    if((Property 'gradient').value.value[0].color[0] -ne 0){throw 'Draft leaked into reset gradient'}
    if((Control 'property-gradient-position').Current.IsEnabled){throw 'Gradient endpoint position should be disabled'}
    Select-Filter 'split_tone' 'Split Tone';Invoke 'property-shadows-color'
    Edit 'property-shadows-color-0' '25';(Control 'property-shadows-color-1').SetFocus()
    Wait-Until {[Math]::Abs((Property 'shadows').value.value[0]-.25) -lt .000001} 'Scalar color not updated'
    if([Math]::Abs((Property 'shadows').value.value[1]-.33) -gt .000001){throw 'Scalar channel edit changed another channel'}
    Capture 'color'
    $theme=(Model).state.theme
    Invoke 'View' -Name
    (Control 'Dark Mode' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    $null=Control 'property-shadows-color'
    Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
    Capture 'alternate-theme'
    Invoke 'Filters' -Name;Edit 'filter-search' 'Curves'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after filter edits and theme not ready' 20
    Invoke 'File' -Name;Invoke 'new_document';Invoke 'Discard Changes' -Name
    Edit 'document-width' '128';Edit 'document-height' '64';Invoke 'Create' -Name
    Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 64 -and !(Model).state.document_file.busy} 'Document replacement failed' 45
    Wait-Until {(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Document dialog gate did not clear'
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Preview after document replacement not ready' 20
    Invoke 'Properties' -Name
    Wait-Until {(Property 'opacity').value.value -eq 1 -and (Property 'blend').value.value -eq 0} 'Replacement reused old property values'
    if((Model).state.document_file.modified){throw 'Preview or stale property changed new document'}
    Invoke 'Filters' -Name
    Wait-Until {(Find 'filter-preview-curves').Current.ItemStatus -eq 'Ready'} 'Retained cache not shown on reopen' 15
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [PSCustomObject]@{
        six_property_kinds='passed';reset_draft_guards_and_endpoints='passed'
        curve_control_retention_and_resize='passed';category_search_and_insertion='passed'
        gpu_preview_paint_and_exact_undo='passed';preview_theme_and_document_replacement='passed'
        clean_document_and_zero_exit='passed'
        scope='isolated native UI and app-only GPU pixels; full workspace visual parity, physical input and presentation acceptance remain separate'
    }|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
