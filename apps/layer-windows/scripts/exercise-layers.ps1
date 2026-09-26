param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCaptureDelay=250
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Drawing;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
public static class CapyLayersCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
    [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
    [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
    public static void Key(uint process,ushort key,ushort modifier=0) {
        uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
        if(owner!=process)throw new Exception("Review does not own keyboard focus; no key sent.");
        var keys=modifier==0?new[]{key}:new[]{modifier,key};var input=new Input[keys.Length*2];
        for(int i=0;i<keys.Length;i++){
            input[i]=new Input{type=1,keyboard=new Keyboard{key=keys[i]}};
            input[input.Length-1-i]=new Input{type=1,keyboard=new Keyboard{key=keys[i],flags=2}};
        }
        if(SendInput((uint)input.Length,input,40)!=input.Length)throw new Exception("Windows rejected the review key.");
    }
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/layers/'+[Guid]::NewGuid().ToString('N'))
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
function Expand([string]$Id){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Type ([System.Windows.Automation.ControlType]::MenuItem);$null -ne $hit.item} "Missing submenu: $Id"
    $hit.item.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
}
function Focus([string]$Id){
    [CapyLayersCapture]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    $item=Control $Id;$item.SetFocus();Wait-Until {$item.Current.HasKeyboardFocus} "Missing focus on $Id"
}
function Toggle-Flag([string]$Id,[switch]$Keyboard){
    $pattern=(Control $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $before=$pattern.Current.ToggleState
    if($Keyboard){Focus $Id;[CapyLayersCapture]::Key($review.Id,0x20)}else{$pattern.Toggle()}
    Wait-Until {$pattern.Current.ToggleState -ne $before} "Accessible toggle state did not change: $Id"
}
function Edit([string]$Id,[string]$Text){
    $entry=Control $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Choose([string]$Id,[string]$Option){
    (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Preview-Hash([string]$Id){
    $image=Control $Id
    if($image.Current.ItemStatus -ne 'Ready'){return ''}
    $r=$image.Current.BoundingRectangle
    $window=[CapyLayersCapture+Rect]::new()
    if(![CapyLayersCapture]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($window.right-$window.left,$window.bottom-$window.top)
    try{
        $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
        try{if(![CapyLayersCapture]::PrintWindow($review.MainWindowHandle,$dc,2)){throw 'App capture failed'}}
        finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
        $area=[Drawing.Rectangle]::new([int]$r.X-$window.left+2,[int]$r.Y-$window.top+2,[int]$r.Width-4,[int]$r.Height-4)
        $crop=$bitmap.Clone($area,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $stream=[IO.MemoryStream]::new();$sha=[Security.Cryptography.SHA256]::Create()
        try{$crop.Save($stream,[Drawing.Imaging.ImageFormat]::Png);[Convert]::ToBase64String($sha.ComputeHash($stream.ToArray()))}
        finally{$crop.Dispose();$stream.Dispose();$sha.Dispose()}
    }finally{$bitmap.Dispose()}
}

try {
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/layers-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned Layers review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyLayersCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    if(!@((Model).layout.groups|Where-Object {$_.active -eq 'layers'}).Count){Invoke 'Layers' -Name}
    $paint=(Model).state.layer_tools.editing_layer.id
    Wait-Until {(Find ("layer-$paint-thumbnail")).Current.ItemStatus -eq 'Ready'} 'Paint thumbnail not ready' 20
    $original=Preview-Hash "layer-$paint-thumbnail";if(!$original){throw 'No initial thumbnail pixels'}
    $identity=(Control "layer-$paint-name").GetRuntimeId() -join ':'
    Capture 'initial'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
    Wait-Until {(Model).state.document_file.modified} 'Stroke not acknowledged'
    Wait-Until {$hash=Preview-Hash "layer-$paint-thumbnail";$hash -and $hash -ne $original} 'Thumbnail did not reflect paint' 15
    Invoke 'Undo' -Name
    Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore checkpoint'
    Wait-Until {(Preview-Hash "layer-$paint-thumbnail") -eq $original} 'Thumbnail pixels did not restore after Undo' 15
    if(((Control "layer-$paint-name").GetRuntimeId() -join ':') -ne $identity){throw 'Painting replaced the layer row'}
    $opacitySlider=Control 'layer-opacity-slider'
    $opacitySlider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.72)
    Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.72) -lt .000001} 'Compact opacity slider did not reach the editing layer'
    Edit 'layer-opacity' '60';(Control 'layer-blend').SetFocus()
    Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.6) -lt .000001} 'Layer opacity not applied'
    Wait-Until {[Math]::Abs((Control 'layer-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).Current.Value-.6) -lt .000001} 'Opacity slider did not follow the numeric field'
    Choose 'layer-blend' 'Multiply'
    Wait-Until {(Model).state.layer_tools.editing_layer.blend -eq 1} 'Layer blend not applied'
    Toggle-Flag 'layer-alpha_lock';Wait-Until {(Model).state.layer_tools.editing_layer.alpha_locked} 'Alpha lock not applied'
    Invoke 'Undo' -Name;Wait-Until {!(Model).state.layer_tools.editing_layer.alpha_locked -and (Control 'layer-alpha_lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off} 'Undo did not restore the accessible alpha-lock state'
    Invoke 'Redo' -Name;Wait-Until {(Model).state.layer_tools.editing_layer.alpha_locked -and (Control 'layer-alpha_lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On} 'Redo did not restore the accessible alpha-lock state'
    Toggle-Flag 'layer-lock' -Keyboard;Wait-Until {(Model).state.layer_tools.editing_layer.locked} 'Edit lock not applied'
    Wait-Until {!(Control 'layer-opacity').Current.IsEnabled -and !(Control 'layer-opacity-slider').Current.IsEnabled -and !(Control 'layer-blend').Current.IsEnabled} 'Locked controls remained enabled'
    Toggle-Flag 'layer-lock' -Keyboard;Wait-Until {!(Model).state.layer_tools.editing_layer.locked} 'Edit lock not cleared'
    $count=(Model).state.layers.Count;Invoke 'layer-new'
    Wait-Until {(Model).state.layers.Count -eq $count+1} 'New layer not created'
    $created=(Model).state.layer_tools.editing_layer.id
    Invoke "layer-$paint-selection"
    Wait-Until {@((Model).state.layers|Where-Object selected).Count -eq 2} 'Independent layer selection failed'
    if((Model).state.layer_tools.editing_layer.id -ne $created){throw 'Selection checkbox changed drawing target'}
    Invoke "layer-$paint-selection"
    Invoke 'layer-add-mask';Wait-Until {(Model).state.layer_tools.editing_layer.has_mask} 'Layer mask not added'
    Wait-Until {(Find "layer-$created-mask-thumbnail").Current.ItemStatus -eq 'Ready'} 'Mask thumbnail not ready' 15
    Invoke "layer-$created-name"
    Wait-Until {!(Model).state.layer_tools.editing_layer.mask_selected} 'Content target not selected'
    foreach($target in @('name','content','mask')){foreach($shift in @($false,$true)){
        $isMask=$target -eq 'mask';$command=if($isMask){'layer-menu-enable_mask'}else{'menu-organize'}
        Focus "layer-$created-$target"
        [CapyLayersCapture]::Key($review.Id,($shift ? 0x79 : 0x5D),($shift ? 0x10 : 0))
        Wait-Until {$item=Find $command;$null -ne $item -and !$item.Current.IsOffscreen -and (Model).state.layer_tools.editing_layer.mask_selected -eq $isMask} "Keyboard opened the wrong layer context: $target, Shift=$shift"
        if($isMask -and $shift){Capture 'keyboard-mask-menu'}
        [CapyLayersCapture]::Key($review.Id,0x1B)
        Wait-Until {$item=Find $command;$null -eq $item -or $item.Current.IsOffscreen} 'Escape did not dismiss the layer menu'
    }}
    Focus "layer-$created-name";[CapyLayersCapture]::Key($review.Id,0x71)
    Wait-Until {(Model).state.layer_tools.rename_layer -eq $created} 'Rename did not begin'
    Edit "layer-$created-rename" 'Native layer';(Control 'layer-blend').SetFocus()
    Wait-Until {(Model).state.layer_tools.editing_layer.label -eq 'Native layer' -and $null -eq (Model).state.layer_tools.rename_layer} 'Rename did not commit'
    Capture 'controls-and-mask'
    # Mask target and protection use the same shared menu as the other ports.
    Invoke "layer-$created-mask"
    Wait-Until {(Model).state.layer_tools.editing_layer.mask_selected} 'Mask target not selected'
    Invoke 'layer-actions'
    (Control 'layer-menu-enable_mask').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {!(Model).state.layer_tools.editing_layer.mask_enabled} 'Mask disable not applied'
    Invoke 'layer-actions'
    (Control 'layer-menu-enable_mask').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).state.layer_tools.editing_layer.mask_enabled} 'Mask enable not applied'
    Invoke "layer-$created-link"
    Wait-Until {!(Model).state.layer_tools.editing_layer.mask_linked} 'Mask unlink not applied'
    Invoke "layer-$created-link"
    Wait-Until {(Model).state.layer_tools.editing_layer.mask_linked} 'Mask link not applied'
    Invoke "layer-$created-name"
    Toggle-Flag 'layer-clip';Wait-Until {(Model).state.layer_tools.editing_layer.clipped} 'Clipping not applied'
    Toggle-Flag 'layer-clip';Wait-Until {!(Model).state.layer_tools.editing_layer.clipped} 'Clipping not cleared'
    Toggle-Flag 'layer-reference';Wait-Until {(Model).state.layer_tools.references_selected} 'Reference selection not applied'
    Toggle-Flag 'layer-reference';Wait-Until {!(Model).state.layer_tools.references_selected} 'Reference selection not cleared'
    Invoke 'layer-actions';Expand 'menu-organize';Invoke 'layer-menu-duplicate'
    Wait-Until {(Model).state.layers.Count -eq $count+2} 'Duplicate did not create a layer'
    Invoke 'layer-delete';Wait-Until {(Model).state.layers.Count -eq $count+1} 'Delete selected did not remove duplicate'
    Invoke 'Undo' -Name;Wait-Until {(Model).state.layers.Count -eq $count+2} 'Undo did not restore duplicate'
    $duplicate=(Model).state.layer_tools.editing_layer.id
    Invoke "layer-$created-selection"
    Wait-Until {@((Model).state.layers|Where-Object selected).Count -eq 2} 'Group selection not ready'
    Invoke 'layer-actions';Expand 'menu-organize';Invoke 'layer-menu-group_selected'
    Wait-Until {@((Model).state.layers|Where-Object {$_.group -and $_.selected}).Count -eq 1} 'Selected layers did not group'
    $group=((Model).state.layers|Where-Object {$_.group -and $_.selected}).id
    if((Model).state.layer_tools.editing_layer.id -ne $duplicate){throw 'Grouping changed the editing target'}
    if(@((Model).state.layers|Where-Object {$_.depth -eq 1}).Count -ne 2){throw 'Group children not indented'}
    Invoke "layer-$created-name";Invoke "layer-$group-content"
    Wait-Until {@((Model).state.layers|Where-Object {$_.id -eq $group -and $_.collapsed}).Count -eq 1} 'Group did not collapse'
    if((Model).state.layer_tools.editing_layer.id -ne $created){throw 'Collapsing group changed the editing target'}
    Edit 'layer-opacity' '47';(Control 'layer-blend').SetFocus()
    Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.47) -lt .000001} 'Hidden editing target lost opacity control'
    Invoke "layer-$group-content"
    Wait-Until {$null -ne (Find "layer-$created-name")} 'Group did not expand'
    Invoke "layer-$group-name";Invoke 'layer-actions';Expand 'menu-organize';Invoke 'layer-menu-ungroup'
    Wait-Until {@((Model).state.layers|Where-Object {$_.id -eq $group}).Count -eq 0} 'Ungroup did not remove container'
    if(@((Model).state.layers|Where-Object {$_.id -in @($created,$duplicate)}).Count -ne 2){throw 'Ungroup lost children'}
    $start=(Model).state.layers.Count
    for($i=1;$i -le 64;$i++){
        Invoke 'layer-new';Wait-Until {(Model).state.layers.Count -eq $start+$i} 'Layer creation stalled'
    }
    $all=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
    $realized=@($all|Where-Object {$_.Current.AutomationId -match '^layer-[0-9]+-name$'})
    if($realized.Count -ge 40){throw 'Layers were not virtualized'}
    $scroll=(Control 'layer-list').GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    $scroll.SetScrollPercent(-1,100)
    Wait-Until {$null -ne (Find "layer-$paint-name")} 'Scroll did not realize old layer'
    Wait-Until {(Find "layer-$paint-thumbnail").Current.ItemStatus -eq 'Ready'} 'Recycled row thumbnail not restored' 15
    $scroll.SetScrollPercent(-1,0)
    $top=(Model).state.layers[0].id
    Wait-Until {$row=Find "layer-$top-name";$row -and !$row.Current.IsOffscreen} 'Scrolling back did not realize the top layer'
    Capture 'virtualized'
    $theme=(Model).state.theme
    Invoke 'settings-button'
    $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=if($theme -eq 'dark'){'Light'}else{'Dark'}
    (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
    $close=$preferences.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Preferences did not close'
    Capture 'alternate-theme'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'new_document'
    $discard=@{item=$null};try{Wait-Until {$discard.item=Find 'Discard Changes' -Name;$null -ne $discard.item -or $null -ne (Find 'document-width')} 'New drawing did not open' 5}catch{}
    if($discard.item){$discard.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    Edit 'document-width' '128';Edit 'document-height' '64';Invoke 'Create' -Name
    Wait-Until {(Model).state.document_file.epoch -gt 0 -and !(Model).state.document_file.busy} 'New document did not replace layer state' 45
    Wait-Until {(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Document gate did not clear'
    $fresh=(Model).state.layer_tools.editing_layer.id
    Wait-Until {(Find "layer-$fresh-thumbnail").Current.ItemStatus -eq 'Ready'} 'Replacement thumbnail not ready' 20
    if((Model).state.layers.Count -ne 2 -or (Model).state.document_file.modified -or (Model).state.layer_tools.editing_layer.opacity -ne 1){throw 'Old layer state leaked into replacement'}
    Capture 'replacement'
    Edit 'layer-opacity' '55'
    $review.CloseMainWindow()|Out-Null
    Wait-Until {$null -ne (Find 'Discard Changes' -Name -Type ([System.Windows.Automation.ControlType]::Button))} 'Closing did not commit the focused draft'
    if(!(Model).state.document_file.modified -or [Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.55) -gt .000001){throw 'Close checked stale document state'}
    Invoke 'Cancel' -Name
    Wait-Until {(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Cancel did not reopen the canvas'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [PSCustomObject]@{thumbnail_paint_and_exact_undo='passed';row_retention='passed';header_and_lock_controls='passed';native_toggle_states_and_history='passed';keyboard_layer_and_mask_menus='passed';independent_selection='passed';mask_thumbnail='passed';rename_duplicate_delete_undo='passed';mask_controls_clipping_references='passed';group_collapse_hidden_target_and_ungroup='passed';virtualized_rows_and_recycling='passed';theme_and_document_replacement='passed';focused_draft_committed_before_close='passed';zero_exit='passed'}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
