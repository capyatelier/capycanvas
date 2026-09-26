param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyExpansionKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Escape(uint process) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no keys sent.");
   var inputs=new[]{new Input{type=1,keyboard=new Keyboard{key=0x1B}},new Input{type=1,keyboard=new Keyboard{key=0x1B,flags=2}}};
   if(SendInput(2,inputs,40)!=2)throw new Exception("Windows rejected Escape.");
 }
 public static void Context(uint process) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no keys sent.");
   var inputs=new[]{new Input{type=1,keyboard=new Keyboard{key=0x10}},new Input{type=1,keyboard=new Keyboard{key=0x79}},
     new Input{type=1,keyboard=new Keyboard{key=0x79,flags=2}},new Input{type=1,keyboard=new Keyboard{key=0x10,flags=2}}};
   if(SendInput(4,inputs,40)!=4)throw new Exception("Windows rejected context-menu keys.");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/expansion/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke $Id
}
function Toolbar([string]$Id){(Model).panels|Where-Object id -eq $Id}
function ToolbarContext([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus();[CapyExpansionKeys]::Context([uint32]$review.Id)
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function DialogButton([string]$Id,[string]$Name){
    $dialog=Control $Id;$found=@{item=$null}
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
    Wait-Until {$found.item=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition);$null -ne $found.item} "Missing dialog button $Name"
    $found.item
}
function InvokeDialog([string]$Id,[string]$Name){(DialogButton $Id $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Configure([string]$Panel,[string]$Source="panel-tab-$Panel"){
    Wait-Until {$null -ne ((Model).panels|Where-Object id -eq $Panel)} "Missing shared panel $Panel"
    $model=(Model).panels|Where-Object id -eq $Panel
    ToolbarContext $Source
    Invoke ($model.configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq $Panel} "Core did not expand $Panel"
    $configuration=Control "panel-configuration-$Panel"
    Wait-Until {$configuration.Current.BoundingRectangle.Width -gt 100} "Configuration $Panel has no native bounds"
    Start-Sleep -Milliseconds 400
    $configuration
}
function Dismiss([string]$Panel,[string]$Source="panel-tab-$Panel"){
    (Control $Source).SetFocus();[CapyExpansionKeys]::Escape([uint32]$review.Id)
    Wait-Until {$null -eq (Model).state.customization.expanded -and $null -eq (Find "panel-configuration-$Panel")} "Escape did not close $Panel configuration"
}
function Toggle([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
function Check-OverviewOverlap($Configuration){
    $overview=(Control 'navigator-overview' -Within $Configuration).Current.BoundingRectangle
    $origin=[CapyExpansionKeys+Point]::new()
    if(![CapyExpansionKeys]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Cannot locate client capture'}
    $scale=[CapyExpansionKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
    $document=(Model).state.tabs[0]
    $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
    $width=$document.width*$fit;$height=$document.height*$fit
    $groups=@((Model).layout.groups|Where-Object {$_.active -ne 'navigator'})
    $hit=$null
    foreach($fy in 0.1,0.3,0.5,0.7,0.9){foreach($fx in 0.04,0.2,0.5,0.8,0.96){
        if($hit){break}
        $px=[int]($overview.Left-$origin.x+($overview.Width-$width)/2+$width*$fx)
        $py=[int]($overview.Top-$origin.y+($overview.Height-$height)/2+$height*$fy)
        foreach($group in $groups){$b=$group.bounds
            if($px -gt $b.x*$scale -and $px -lt ($b.x+$b.width)*$scale -and $py -gt ($b.y+36)*$scale -and $py -lt ($b.y+$b.height)*$scale){$hit=@{x=$px;y=$py};break}}
    }}
    if(!$hit){throw 'Fixture did not overlap a lower native panel'}
    $x=$hit.x;$y=$hit.y
    $before=[Drawing.Bitmap]::new((Join-Path $run 'navigator-before.png'))
    $after=[Drawing.Bitmap]::new((Join-Path $run 'navigator.png'))
    try{
        $old=$before.GetPixel($x,$y);$pixel=$after.GetPixel($x,$y)
        if($old.R -gt 245 -and $old.G -gt 245 -and $old.B -gt 245){throw 'Overlap reference was already white'}
        if($pixel.R -lt 245 -or $pixel.G -lt 245 -or $pixel.B -lt 245){throw 'Lower native panel covers the GPU overview'}
        return @{x=$x;y=$y;before=$old.ToArgb()}
    }finally{$before.Dispose();$after.Dispose()}
}
function Shown([string]$Panel,[string]$Control){((Model).panels|Where-Object id -eq $Panel).controls|Where-Object control -eq $Control|Select-Object -ExpandProperty visible_in_panel}
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/expansion-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned expansion review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyExpansionKeys]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $configuration=Configure 'sizes'
    $identity=(Control 'configure-show-brush_size').GetRuntimeId() -join ':'
    $scale=(Control 'panel-tab-sizes').Current.BoundingRectangle.Height/36
    if([Math]::Abs($configuration.Current.BoundingRectangle.Width-380*$scale) -gt 2){throw 'Configuration does not use the shared 380 DIP width'}
    $before=(Model).state.brush.diameter
    (Control 'configure-brush_size-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.35)
    Wait-Until {(Model).state.brush.diameter -ne $before} 'Configuration slider did not update shared brush size'
    Toggle 'configure-show-brush_size'
    Wait-Until {!(Shown 'sizes' 'brush_size')} 'Visibility checkbox did not hide the main brush size control'
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Visibility change replaced configuration controls'}
    Toggle 'configure-show-brush_size'
    Wait-Until {Shown 'sizes' 'brush_size'} 'Visibility checkbox did not restore the main brush size control'
    Capture 'sizes'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Wait-Until {(Model).state.customization.expanded -eq 'sizes'} 'Resize dismissed configuration'
    Start-Sleep -Milliseconds 400
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Resize replaced configuration controls'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 720
    Start-Sleep -Milliseconds 400
    $placement=$configuration.Current.ItemStatus|ConvertFrom-Json
    if(!$placement -or $placement.configuration.width -gt 380){throw 'Narrow expansion lost shared geometry'}
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Narrow resize replaced configuration controls'}
    Capture 'sizes-narrow'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Start-Sleep -Milliseconds 400
    Dismiss 'sizes'
    $configuration=Configure 'layers' $(if(Find 'panel-tab-layers'){'panel-tab-layers'}else{'column-icon-layers'})
    $original=(Model).state.layer_tools.editing_layer.id
    Invoke (((Model).state.commands|Where-Object id -eq 'add_layer').label) -Name -Within $configuration
    Wait-Until {(Model).state.layer_tools.editing_layer.id -ne $original} 'Configuration layer action did not create a layer'
    $created=(Model).state.layer_tools.editing_layer.id
    $selector=Control 'configure-control-layers'
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Current ink' -Name -Within $selector).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $original} 'Configuration selector did not change the shared editing layer'
    (Control 'configure-layer-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
    Wait-Until {(Model).state.layer_tools.editing_layer.opacity -eq 0.5} 'Configuration opacity did not affect the selected layer'
    if(((Model).state.layers|Where-Object id -eq $created).opacity -ne 1){throw 'Configuration opacity changed the previous target'}
    Toggle 'configure-show-layer_opacity'
    Wait-Until {!(Shown 'layers' 'layer_opacity')} 'Layer opacity visibility did not change'
    Toggle 'configure-show-layer_opacity'
    Wait-Until {Shown 'layers' 'layer_opacity'} 'Layer opacity visibility did not restore'
    Capture 'layers'
    Dismiss 'layers'
    # Navigator is already present in the full editor preset.
    $null=Control 'panel-tab-navigator'
    # Constrain the full editor so the configuration's GPU overview overlaps
    # the opaque Tool Set body; the wide preset leaves empty canvas below it.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1100 -Height 1000
    Start-Sleep -Milliseconds 400
    Capture 'navigator-before'
    $configuration=Configure 'navigator'
    $null=Control 'navigator-overview' -Within $configuration
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the review'
    $zoom=(Control 'canvas-camera').Current.Name
    Invoke 'navigator-zoom_in' -Within $configuration
    Wait-Until {(Control 'canvas-camera').Current.Name -ne $zoom} 'Configuration Navigator did not update the shared camera'
    Start-Sleep -Milliseconds 250
    Capture 'navigator'
    $occlusion=Check-OverviewOverlap $configuration
    Dismiss 'navigator'
    Start-Sleep -Milliseconds 250
    Capture 'navigator-after'
    $restored=[Drawing.Bitmap]::new((Join-Path $run 'navigator-after.png'))
    try{$pixel=$restored.GetPixel($occlusion.x,$occlusion.y);if($pixel.R -gt 245 -and $pixel.G -gt 245 -and $pixel.B -gt 245){throw 'Closing configuration did not restore the lower native panel'}}
    finally{$restored.Dispose()}
    $configuration=Configure 'sizes'
    Invoke 'settings-button'
    $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Light' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq 'light'} 'Theme preference did not update'
    (Control 'Close' -Name -Within $preferences -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Preferences did not release the canvas'
    if(!(Model).state.customization.expanded){$configuration=Configure 'sizes'}
    $null=Control 'panel-configuration-sizes'
    Start-Sleep -Milliseconds 400
    Capture 'sizes-light'
    Dismiss 'sizes'
    $configuration=Configure 'toolbar' 'ribbon-grip-toolbar'
    Invoke 'Add Tools…' -Name -Within $configuration
    $null=Control 'tool-picker'
    InvokeDialog 'tool-picker' 'Cancel'
    Wait-Until {$null -eq (Find 'tool-picker')} 'Expansion Add Tools picker did not close'
    if((Model).state.customization.expanded){Dismiss 'toolbar' 'ribbon-grip-toolbar'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{shared_expansion_width='passed';live_brush_size='passed';control_visibility='passed';retained_configuration='passed';resize='passed';escape_close='passed';layers='passed';editing_layer_selection='passed';targeted_opacity='passed';navigator_configuration='passed';gpu_preview_occlusion_and_restore='passed';theme='passed';toolbar_add_tools='passed';zero_exit='passed';scope='native configuration projection; full visual parity, physical input and presentation are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
