param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','layout-dark','layout-light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$layoutOnly=$Theme.StartsWith('layout-');if($layoutOnly){$Theme=$Theme.Substring(7)}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyScopeControls {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
    [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,StringBuilder text,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
    public static void TypeText(IntPtr edit,string text) {
        UIntPtr result;
        if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
        if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
        foreach(char c in text)
            if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
        var actual=new StringBuilder(32768);if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot verify native picker filename");
        if(actual.ToString()!=text)throw new Exception("The native picker filename did not match the test path");
    }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/scopes/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$lookup=Join-Path $run 'Fixture look.cube'
[IO.File]::WriteAllText($lookup,"TITLE `"Fixture look`"`nLUT_3D_SIZE 2`n0 0 0`n1 0 0`n0 1 0`n1 1 0`n0 0 1`n1 0 1`n0 1 1`n0.9 0.8 0.6`n")
function Invoke([string]$Value,[switch]$Name){
    $hit=@{item=$null}
    Wait-Until {
        $hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::Button)
        if(!$hit.item){$hit.item=Find $Value -Name:$Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $null -ne $hit.item
    } "Missing action: $Value"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Edit([string]$Id,[string]$Text){$entry=Control $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)}
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
    if($null -ne (Model).state.filter_picker.search){Invoke 'filter-search-toggle'}
    Choose 'filter-category' 'All filters'
    Invoke 'filter-search-toggle'
    Edit 'filter-search' $Label
    Wait-Until {(Model).state.filter_picker.search -eq $Label -and $null -ne (Find ('filter-'+$Id))} 'Filter search did not reach the native list'
    Invoke ('filter-'+$Id)
    Wait-Until {(Model).state.layer_properties.description -eq $Label} "$Label did not become the active layer"
    Select-Panel 'properties'
}
function Workspace([string]$Id){
    if((Model).windows_workspace.id -eq $Id){return}
    $choice=@((Model).windows_workspace.switcher|Where-Object id -eq $Id)[0]
    $toggle=Find ('workspace-switch-'+$Id) -Visible
    if(!$toggle){Invoke 'header-workspace-menu';$toggle=Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $toggle.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} 'Workspace did not open'
}
function Center($Element){$box=$Element.Current.BoundingRectangle;@([int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2))}
function Tap($Element,[string]$Device){
    if($script:lastDevice -and $script:lastDevice -ne $Device){Start-Sleep -Milliseconds 500};$script:lastDevice=$Device
    $at=Center $Element
    if($Device -eq 'mouse'){[CapyRowPointer]::Hover($at[0],$at[1]);Start-Sleep -Milliseconds 80}
    [CapyRowPointer]::Down($Device,$at[0],$at[1]);Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 120
}
function Tap-Id([string]$Id,[string]$Device){Tap (Control $Id -Arranged) $Device}
function Pick([string]$Id,[string]$Option,[string]$Device){
    Tap-Id $Id $Device
    $item=Control $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Arranged
    Tap $item $Device
    Wait-Until {!(Find $Option -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Visible)} "$Id did not close after choosing $Option"
}
function View([string]$Name){(Model).state.$Name}
function Window-Capture{
    $window=[CapyScopeControls+Rect]::new()
    if(![CapyScopeControls]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($window.right-$window.left,$window.bottom-$window.top)
    $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
    try{if(![CapyScopeControls]::PrintWindow($review.MainWindowHandle,$dc,2)){$bitmap.Dispose();throw 'App capture failed'}}finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
    @{bitmap=$bitmap;window=$window}
}
function Reveal([string]$Id){
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$node=$walker.GetParent((Control $Id));$scroll=$null
    while($node -and !($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll) -and $scroll.Current.VerticallyScrollable)){$node=$walker.GetParent($node);$scroll=$null}
    if(!$node){return $null}
    $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0);Start-Sleep -Milliseconds 200
    $view=$node.Current.BoundingRectangle;$target=(Control $Id).Current.BoundingRectangle
    $range=$view.Height*(100/$scroll.Current.VerticalViewSize-1)
    if($range -gt 0 -and $target.Bottom -gt $view.Bottom){
        $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,[Math]::Min(100,100*($target.Bottom-$view.Bottom+4)/$range))
        Wait-Until {(Control $Id).Current.BoundingRectangle.Bottom -le $view.Bottom+1} "$Id did not scroll into view"
    }
    $view
}
function Chart-Shades([string]$Id){
    $view=Reveal $Id
    $r=(Control $Id).Current.BoundingRectangle
    if($view){$r=[System.Windows.Rect]::Intersect($r,$view)}
    if($r.IsEmpty -or $r.Height -lt 24){throw "$Id is not visible"}
    $capture=Window-Capture;$bitmap=$capture.bitmap;$window=$capture.window
    try{
        $shades=@{}
        for($y=[int]$r.Top-$window.top+4;$y -lt [int]$r.Bottom-$window.top-4;$y+=3){for($x=[int]$r.Left-$window.left+4;$x -lt [int]$r.Right-$window.left-4;$x+=3){$shades[$bitmap.GetPixel($x,$y).ToArgb()]=1}}
        $shades.Count
    }finally{$bitmap.Dispose()}
}
function Window-Panel([string]$Name){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $menuItem=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.OrCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,"$Name panel"),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name)),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    Wait-Until {$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuItem)} "$Name is missing from the Window menu"
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuItem).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
}
function Shown([string]$Panel){@((Model).layout.groups|Where-Object {$_.panels -contains $Panel}).Count -gt 0}
function Canvas-Hash{
    $away=Center (Control 'panel-tab-properties');[CapyRowPointer]::Hover($away[0],$away[1])
    $from=Canvas-Point .15 .3;$to=Canvas-Point .85 .7
    $settled=@{hash=$null}
    Wait-Until {
        $capture=Window-Capture
        try{
            $area=[Drawing.Rectangle]::FromLTRB($from[0]-$capture.window.left,$from[1]-$capture.window.top,$to[0]-$capture.window.left,$to[1]-$capture.window.top)
            $crop=$capture.bitmap.Clone($area,[Drawing.Imaging.PixelFormat]::Format32bppArgb);$stream=[IO.MemoryStream]::new();$sha=[Security.Cryptography.SHA256]::Create()
            try{$crop.Save($stream,[Drawing.Imaging.ImageFormat]::Png);$hash=[Convert]::ToBase64String($sha.ComputeHash($stream.ToArray()))}finally{$crop.Dispose();$stream.Dispose();$sha.Dispose()}
        }finally{$capture.bitmap.Dispose()}
        $same=$hash -eq $settled.hash;$settled.hash=$hash;if(!$same){Start-Sleep -Milliseconds 400};$same
    } 'The canvas did not settle'
    $settled.hash
}
function Curve-Json{ConvertTo-Json -InputObject (@((Model).state.layer_properties.controls|Where-Object key -eq 'rgb')[0].value.value) -Compress -Depth 6}
function Properties-Json{(Model).state.layer_properties.controls|ConvertTo-Json -Depth 12 -Compress}
function Undo-To([string]$Before,[string]$Reason){
    Invoke 'Undo' -Name;Wait-Until {(Properties-Json) -eq $Before} "$Reason was not one Undo step"
}
function Canvas-Point([double]$X,[double]$Y){
    $canvas=Control 'drawing-canvas' -Arranged
    Wait-Until {$canvas.Current.IsEnabled -and (Model).brush_ready -and !(Find 'canvas-status' -Visible)} 'The canvas did not become ready for a drag'
    $bounds=$canvas.Current.BoundingRectangle;$camera=(Model).state.camera;$area=$camera.work_area
    $scale=$bounds.Width/$camera.viewport[0]
    @([int]($bounds.X+($area[0]+$area[2]*$X)*$scale),[int]($bounds.Y+($area[1]+$area[3]*$Y)*$scale))
}
function Drag-Canvas([string]$Device,$From,$To){
    [CapyRowPointer]::Down($Device,$From[0],$From[1])
    try{for($step=1;$step -le 12;$step++){[CapyRowPointer]::Move([int]($From[0]+($To[0]-$From[0])*$step/12),[int]($From[1]+($To[1]-$From[1])*$step/12));Start-Sleep -Milliseconds 16}}finally{[CapyRowPointer]::Up()}
    Start-Sleep -Milliseconds 150
}
function Picker([string]$Name){
    $script:picker=Control $Name -Name -Within $root -Type ([System.Windows.Automation.ControlType]::Window) -Seconds 30
    if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $review.Id){throw 'Picker does not belong to the isolated review'}
}
function Choose-Path([string]$Path){
    $hit=@{entry=$null}
    Wait-Until {
        $hit.entry=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
        $null -ne $hit.entry
    } 'Missing picker filename'
    if($hit.entry.Current.ProcessId -ne $review.Id){throw 'Wrong picker filename owner'}
    [CapyScopeControls]::TypeText([IntPtr]$hit.entry.Current.NativeWindowHandle,$Path)
    $accept=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
    $handle=[IntPtr]$accept.Current.NativeWindowHandle;$owner=[uint32]0;[CapyScopeControls]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Picker button has an unexpected owner'}
    if(![CapyScopeControls]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot accept the picker'}
}
function Scope-Language([string]$Tag,[int]$Index){
    $caption=@((Model).application_menus|Where-Object id -eq 'edit')[0].label
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit' -Caption $caption
    Invoke-Id 'settings'
    Wait-Until {(Model).preferences -and (Find 'preference-choice-language')} 'Language preference did not open'
    $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
    Choose 'preference-choice-language' $row.kind.options[$Index]
    Wait-Until {(Model).windows_active_tag -eq $tag} "Language $tag did not apply" 30
    Invoke-Id 'CloseButton'
    Wait-Until {!(Model).preferences} 'Preferences did not close'
}
function Localized-Layout{
    $state=State-File
    $bootstrap=Join-Path (Split-Path -Parent $state) ((Split-Path -Leaf $state) -replace '^ui-state-','bootstrap-')
    $tags=@((Read-Snapshot $bootstrap).shipped_tags)
    if(!$tags.Count){throw 'The native bootstrap has no registered languages'}
    $seen=@()
    for($index=0;$index -lt $tags.Count;$index++){
        Scope-Language $tags[$index] ($index+1)
        $tag=$tags[$index]
        foreach($width in 1100,1500){
            & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width $width -Height 1000
            foreach($prefix in 'histogram','waveform'){
                Select-Panel $prefix
                $null=Reveal ($prefix+'-highlights')
                $plot=(Control ($prefix+'-chart') -Arranged).Current.BoundingRectangle
                $log=Control ($prefix+'-log') -Arranged
                $status=Control ($prefix+'-status') -Arranged
                $bounds=$log.Current.BoundingRectangle;$footer=$status.Current.BoundingRectangle
                if($bounds.Bottom -gt $footer.Top+1){throw "$tag $prefix at $width pixels crowds Log counts into the precision row"}
                foreach($id in 'log','status','shadows','highlights'){
                    $box=(Control ($prefix+'-'+$id) -Arranged).Current.BoundingRectangle
                    if($box.Left -lt $plot.Left-1 -or $box.Right -gt $plot.Right+1 -or $box.Width -le 0){throw "$tag $prefix-$id at $width pixels exceeds the plot width"}
                }
                Wait-Until {
                    $view=View $prefix;$log=Find ($prefix+'-log');$status=Find ($prefix+'-status')
                    $log -and $status -and $log.Current.Name -eq $view.labels[0] -and $status.Current.Name -eq $view.status
                } "$tag $prefix has stale translated controls"
                Capture "$tag-$prefix-$width-$Theme" -Composed -WithModel
            }
        }
        $seen+=$tag
    }
    $checks.localized_layout=@{languages=$seen;widths=@(1100,1500);panels=@('histogram','waveform')}
    Scope-Language 'en' ([array]::IndexOf($tags,'en')+1)
}
$checks=[ordered]@{}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Write-Output "Owned scopes review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Review did not start' 120
    if(!(Model).windows_isolated_settings){throw 'Scopes fixture requires an isolated profile'}
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Workspace 'builtin:workspace:photographer'
    $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'histogram'})[0]
    if(!$group -or $group.panels -notcontains 'waveform' -or $group.active -ne 'histogram'){throw 'Photo did not open Histogram with Waveform in the adjacent tab'}
    $null=Control 'histogram-source' -Arranged
    $checks.photo_layout='passed'

    $fitRevision=(Model).state.camera.revision
    Fit-Canvas
    Wait-Until {(Model).state.camera.revision -gt $fitRevision} 'Fit did not update the view'
    $settled=@{area=$null};Wait-Until {$area=(Model).state.camera.work_area|ConvertTo-Json -Compress;$same=$area -eq $settled.area;$settled.area=$area;Start-Sleep -Milliseconds 150;$same} 'The fitted view did not settle'
    Invoke (Tool-Tile 'gradient')
    Wait-Until {((Model).state.commands|Where-Object id -eq 'gradient').selected} 'Gradient tool did not activate'
    Drag-Canvas 'mouse' (Canvas-Point .1 .5) (Canvas-Point .9 .5)
    Wait-Until {(Model).state.document_file.modified} 'The gradient did not reach the drawing'
    Wait-Until {$h=View 'histogram';$null -ne $h.data -and $h.status -and (Model).windows_scopes -gt 0} 'Histogram did not publish counts'
    Wait-Until {(Chart-Shades 'histogram-chart') -gt 3} 'Histogram did not draw its counts'
    $chart=Control 'histogram-chart'
    if($chart.Current.Name -ne (View 'histogram').description -or $chart.Current.HelpText -ne (View 'histogram').range){throw 'The graph does not expose the shared counts'}
    if($layoutOnly){
        Localized-Layout
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
        [pscustomobject]@{theme=$Theme;checks=$checks;evidence=$run}|ConvertTo-Json -Depth 6
        return
    }
    $revision=(Model).state.document_file.revision
    $sources=@((View 'histogram').sources);$channels=@((View 'histogram').channels)
    Pick 'histogram-source' $sources[3] 'touch';Wait-Until {(View 'histogram').source -eq 3} 'Touch did not choose Selection'
    Pick 'histogram-source' $sources[0] 'mouse';Wait-Until {(View 'histogram').source -eq 0} 'Mouse did not restore Visible'
    Pick 'histogram-channel' $channels[1] 'pen';Wait-Until {(View 'histogram').channel -eq 1} 'Pen did not choose Red'
    Wait-Until {(Chart-Shades 'histogram-chart') -gt 2} 'Red counts were not drawn'
    Pick 'histogram-channel' $channels[0] 'mouse';Wait-Until {(View 'histogram').channel -eq 0} 'Mouse did not restore RGB'
    $checks.source_and_channel='passed'
    Tap-Id 'histogram-log' 'pen';Wait-Until {(View 'histogram').logarithmic} 'Pen did not enable Log counts'
    Tap-Id 'histogram-log' 'touch';Wait-Until {!(View 'histogram').logarithmic} 'Touch did not disable Log counts'
    if((Control 'histogram-log').Current.Name -ne (View 'histogram').labels[0]){throw 'Log counts does not use the shared label'}
    foreach($pass in @(@('histogram-shadows','shadows','touch'),@('histogram-highlights','highlights','mouse'))){
        Tap-Id $pass[0] $pass[2];Wait-Until {(View 'histogram').($pass[1])} "$($pass[1]) did not turn on"
        Tap-Id $pass[0] 'pen';Wait-Until {!(View 'histogram').($pass[1])} "$($pass[1]) did not turn off"
    }
    if((Model).state.document_file.revision -ne $revision){throw 'Scope controls changed the drawing'}
    $checks.log_and_clipping='passed'

    Select-Panel 'waveform'
    Wait-Until {$null -ne (View 'waveform').data} 'Waveform did not publish counts'
    Wait-Until {(Chart-Shades 'waveform-chart') -gt 3} 'Waveform did not draw its trace'
    $waveformChannels=@((View 'waveform').channels)
    Pick 'waveform-channel' $waveformChannels[4] 'touch';Wait-Until {(View 'waveform').channel -eq 4} 'Touch did not choose Luminance'
    if((View 'histogram').channel -ne 0){throw 'Waveform channel changed the Histogram channel'}
    $traceLog=(View 'waveform').logarithmic
    Tap-Id 'waveform-log' 'mouse';Wait-Until {(View 'waveform').logarithmic -ne $traceLog} 'Waveform Log counts did not apply'
    if((View 'histogram').logarithmic){throw 'Waveform Log counts changed the Histogram'}
    Tap-Id 'waveform-log' 'pen';Wait-Until {(View 'waveform').logarithmic -eq $traceLog} 'Waveform Log counts did not restore'
    Tap-Id 'waveform-shadows' 'pen';Wait-Until {(View 'histogram').shadows} 'Waveform clipping did not share the Histogram toggle'
    Tap-Id 'waveform-shadows' 'mouse';Wait-Until {!(View 'histogram').shadows} 'Waveform clipping did not turn off'
    Select-Panel 'histogram'
    $checks.waveform='passed'
    foreach($panel in @(@('Waveform','waveform'),@('Histogram','histogram'))){
        Window-Panel $panel[0];Wait-Until {!(Shown $panel[1])} "Window did not hide $($panel[0])"
    }
    if((Find 'histogram-source' -Visible) -or (Find 'waveform-source' -Visible)){throw 'Hidden scopes left their controls loaded'}
    $checks.window_hide='passed'

    Select-Filter 'levels' 'Levels'
    Wait-Until {(Model).state.layer_properties.histogram -and $null -ne (View 'tonal_histogram').data} 'Levels did not publish input statistics'
    Wait-Until {(Chart-Shades 'tonal-chart') -gt 3} 'Levels did not draw its input statistics'
    $before=Properties-Json
    Tap-Id 'property-action-auto_levels' 'mouse'
    Wait-Until {(Properties-Json) -ne $before} 'Auto did not adjust Levels'
    Undo-To $before 'Auto'
    $pixels=Canvas-Hash
    foreach($device in @('mouse','touch','pen')){
        $revision=(Model).state.document_file.revision
        Tap-Id 'property-action-group-calibration' $device
        Tap-Id 'property-action-calibrate-white' $device
        Wait-Until {(Model).state.color_picker.calibrating} "$device did not arm the white point"
        if(Find 'picker-setting-source' -Visible){throw 'Sampler Source stayed visible while calibrating'}
        $at=Canvas-Point .3 .5;$script:lastDevice=$device;[CapyRowPointer]::Down($device,$at[0],$at[1]);Start-Sleep -Milliseconds $(if($device -eq 'touch'){1500}else{60});[CapyRowPointer]::Up()
        Wait-Until {(Model).state.document_file.revision -gt $revision -and !(Model).state.color_picker.calibrating} "$device white point did not adjust Levels"
        if((Canvas-Hash) -eq $pixels){throw "$device white point did not change the drawing"}
        Invoke 'Undo' -Name;Wait-Until {(Properties-Json) -eq $before} "$device white point undo did not restore Levels"
        if((Canvas-Hash) -ne $pixels){throw "$device white point was not one Undo step"}
    }
    $checks.levels='passed'

    Select-Filter 'curves' 'Curves'
    Wait-Until {$null -ne (View 'tonal_histogram').data} 'Curves did not publish input statistics'
    Wait-Until {(Chart-Shades 'property-rgb-curve') -gt 4} 'Curves did not draw its input statistics'
    if((Control 'curve-status').Current.Name -ne (View 'tonal_histogram').status){throw 'Curves status differs from the shared statistics'}
    $before=Curve-Json
    Tap-Id 'property-action-target_curve' 'touch'
    $from=Canvas-Point .5 .5;$to=@($from[0],($from[1]-80))
    Drag-Canvas 'mouse' $from $to
    Wait-Until {(Curve-Json) -ne $before} 'A targeted drag did not change the curve'
    $settled=@{json=$null};Wait-Until {$json=Curve-Json;$same=$json -eq $settled.json;$settled.json=$json;Start-Sleep -Milliseconds 300;$same} 'The targeted drag did not finish'
    Tap-Id 'property-action-target_curve' 'pen'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Leaving targeted adjustment did not make Undo available'
    Invoke 'Undo' -Name;Wait-Until {(Curve-Json) -eq $before} 'Targeted drag was not one Undo step'
    $checks.targeted_curves='passed'

    Select-Filter 'color_lookup' 'Color Lookup (LUT)'
    $properties=(Model).state.layer_properties
    $warm=@($properties.actions|Where-Object {$_.action.op -eq 'lookup_preset' -and $_.action.preset -eq 'warm'})[0]
    if(!$warm){throw 'Color Lookup has no Warm preset'}
    $before=Properties-Json;$selection=$properties.resource_selection
    Pick 'property-resource-choice' $warm.label 'mouse'
    Wait-Until {$s=(Model).state.layer_properties.resource_selection;$null -ne $s -and $s -ne $selection} 'Warm did not apply'
    Invoke 'Undo' -Name;Wait-Until {(Model).state.layer_properties.resource_selection -eq $selection} 'Warm was not one Undo step'
    Tap-Id 'property-action-import_lookup' 'pen'
    Picker 'Open';Choose-Path $lookup
    Wait-Until {$p=(Model).state.layer_properties;$p.resource_name -eq 'Fixture look' -and $null -eq $p.resource_selection} 'The imported lookup table did not apply'
    Wait-Until {(Control 'property-resource-choice').Current.HelpText -eq 'Fixture look'} 'The selector did not name the imported table'
    Invoke 'Undo' -Name;Wait-Until {(Model).state.layer_properties.resource_selection -eq $selection} 'Import was not one Undo step'
    $checks.color_lookup='passed'

    Window-Panel 'Histogram';Wait-Until {Shown 'histogram'} 'Window did not show Histogram again'
    $null=Control 'histogram-source' -Arranged
    Wait-Until {$null -ne (View 'histogram').data} 'Reopened Histogram did not publish counts'
    $checks.window_show='passed'

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    [pscustomobject]@{theme=$Theme;checks=$checks;devices=@('mouse','pen','touch');evidence=$run}|ConvertTo-Json -Depth 4
}catch{
    try{$capture=Window-Capture;try{$capture.bitmap.Save((Join-Path $run 'failure.png'))}finally{$capture.bitmap.Dispose()}}catch{}
    try{(Model).state|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
