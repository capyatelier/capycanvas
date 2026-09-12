param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyEditorKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wParam,IntPtr lParam);
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Tab(uint process) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no keys sent.");
   var inputs=new[]{new Input{type=1,keyboard=new Keyboard{key=0x09}},new Input{type=1,keyboard=new Keyboard{key=0x09,flags=2}}};
   if(SendInput(2,inputs,40)!=2)throw new Exception("Windows rejected Tab.");
 }
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
$run=Join-Path $repo ('artifacts/windows/editor/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$script:lastModel=$s.model}}catch{}
    $script:lastModel
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Predicate){return};$review.Refresh();if($review.HasExited){throw 'Editor review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){(Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    Invoke 'application-menu-window';Invoke $Id
}
function Toolbar([string]$Id){(Model).panels|Where-Object id -eq $Id}
function ToolbarContext([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus();[CapyEditorKeys]::Context([uint32]$review.Id)
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
function Set-Viewport {
    # Match Chrome's integer CSS viewport at the actual monitor scale. Client
    # and non-client dimensions differ, so adjust the window by the measured
    # canvas delta instead of assuming a fixed frame thickness.
    for($attempt=0;$attempt -lt 3;$attempt++){
        $scale=[CapyEditorKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
        $canvas=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
        $dx=[Math]::Round(986*$scale-$canvas.Width);$dy=[Math]::Round(658*$scale-$canvas.Height)
        if([Math]::Abs($dx) -lt 1 -and [Math]::Abs($dy) -lt 1){return}
        $window=[CapyEditorKeys+Rect]::new()
        if(![CapyEditorKeys]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Cannot measure review window'}
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width ($window.right-$window.left+$dx) -Height ($window.bottom-$window.top+$dy)
        Start-Sleep -Milliseconds 350
    }
    throw 'Native viewport did not match the Chrome fixture'
}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
    $scale=[CapyEditorKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
    $canvas=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
    $origin=[CapyEditorKeys+Point]::new()
    if(![CapyEditorKeys]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Cannot locate client origin'}
    $bitmap=[Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{
        [IO.File]::WriteAllText((Join-Path $run ($Name+'-metrics.json')),(@{
            viewport=@(($canvas.Width/$scale),($canvas.Height/$scale));scale=$scale;
            viewport_origin_physical=@(($canvas.Left-$origin.x),($canvas.Top-$origin.y));
            client_capture=@($bitmap.Width,$bitmap.Height);camera=(Control 'canvas-camera').Current.Name
        }|ConvertTo-Json))
    }finally{$bitmap.Dispose()}
}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id}
function Set-Zen {
    $button=Find (Command 'zen_mode').label -Name -Type ([System.Windows.Automation.ControlType]::Button)
    if($button){$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    else{(Control 'Drawing canvas' -Name).SetFocus();[CapyEditorKeys]::Tab([uint32]$review.Id)}
}
function Select-Command([string]$Id,[switch]$Zen){
    foreach($panel in (Model).panels){
        $tile=$panel.tiles|Where-Object {$_.control.kind -eq 'command' -and $_.control.command -eq $Id}|Select-Object -First 1
        if($tile){$prefix=if($Zen){'zen-tile'}else{'tile'};Invoke "$prefix-$($panel.id)-$($tile.id)";return}
    }
    throw "No native tile for $Id"
}
function Check-Rect($Control,$Box,[double]$X=0,[double]$Y=0){
    $origin=[CapyEditorKeys+Point]::new()
    if(![CapyEditorKeys]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Cannot locate client origin'}
    $scale=[CapyEditorKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
    $actual=$Control.Current.BoundingRectangle
    $expected=@(($origin.x+($X+$Box.x)*$scale),($origin.y+($Y+$Box.y)*$scale),($Box.width*$scale),($Box.height*$scale))
    $values=@($actual.Left,$actual.Top,$actual.Width,$actual.Height)
    for($i=0;$i -lt 4;$i++){
        if([Math]::Abs($values[$i]-$expected[$i]) -gt 2){throw "Native bounds differ from Core: $($Control.Current.AutomationId) [$values] vs [$expected]"}
    }
}
function Check-Editor {
    $model=Model
    foreach($group in $model.layout.groups){
        $panel=$model.panels|Where-Object id -eq $group.active
        if($group.tabs_visible){
            $first=$group.panels[0];$tab=Control "panel-tab-$first"
            $measure=$model.panel_measurements|Where-Object panel -eq $first
            Check-Rect $tab @{x=0;y=0;width=$measure.tab_width;height=36} $group.bounds.x $group.bounds.y
            Check-Rect (Control "group-grip-$($group.id)") @{x=($group.bounds.width-28);y=0;width=20;height=36} $group.bounds.x $group.bounds.y
        }
        if($group.tiles){
            for($i=0;$i -lt $panel.tiles.Count;$i++){
                if($panel.tiles[$i].control.kind -eq 'divider'){continue}
                $tile=Control "tile-$($panel.id)-$($panel.tiles[$i].id)"
                $tabHeight=if($group.tabs_visible){36}else{0}
                Check-Rect $tile $group.tiles.tiles[$i] $group.bounds.x ($group.bounds.y+$tabHeight)
            }
        }
    }
}
function Check-Zen {
    $model=Model
    if(!$model.partial_zen -or !$model.chrome_hidden -or !$model.zen_toolbars.sections.Count){throw 'Partial Zen projection is missing'}
    $insets=$model.titlebar_insets
    if($insets.Count -ne 3 -or $insets[1] -le 0){throw 'Native caption measurements did not reach Core'}
    foreach($section in $model.zen_toolbars.sections){
        if($section.bounds.y -lt $insets[2] -and $section.bounds.width -gt 0){
            $scale=[CapyEditorKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
            $viewport=(Control 'Drawing canvas' -Name).Current.BoundingRectangle.Width/$scale
            if($section.bounds.x -lt $insets[0] -or $section.bounds.x+$section.bounds.width -gt $viewport-$insets[1]){throw 'Zen overlaps native caption controls'}
        }
        foreach($pair in $section.tiles){
            $box=$pair[1]
            if($box.x+$box.width -gt $section.bounds.width -or $box.y+$box.height -gt $section.bounds.height){continue}
            $native=Control "zen-tile-$($section.panel)-$($pair[0])"
            Check-Rect $native $box $section.bounds.x $section.bounds.y
            if($section.bounds.y -lt $insets[2]){
                $bounds=$native.Current.BoundingRectangle
                $x=[int]($bounds.Left+$bounds.Width/2);$y=[int]($bounds.Top+$bounds.Height/2)
                $point=[IntPtr](([int64]($y -band 65535) -shl 16) -bor ($x -band 65535))
                if([CapyEditorKeys]::SendMessage($review.MainWindowHandle,0x84,[IntPtr]::Zero,$point).ToInt64() -ne 1){throw 'Zen control is inside a native window-drag region'}
            }
        }
    }
}
function Check-Header {
    $scale=[CapyEditorKeys]::GetDpiForWindow($review.MainWindowHandle)/96.
    $menu=(Control 'application-menu-help').Current.BoundingRectangle
    $settings=(Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Button)).Current.BoundingRectangle
    $points=@(
        @{x=$menu.Left+$menu.Width/2;y=$menu.Top+$menu.Height/2;expected=1;name='Help menu'},
        @{x=$settings.Left+$settings.Width/2;y=$settings.Top+$settings.Height/2;expected=1;name='Preferences'},
        @{x=$menu.Right+4*$scale;y=$menu.Top+$menu.Height/2;expected=2;name='unused header space'}
    )
    foreach($entry in $points){
        $x=[int]$entry.x;$y=[int]$entry.y
        $point=[IntPtr](([int64]($y -band 65535) -shl 16) -bor ($x -band 65535))
        if([CapyEditorKeys]::SendMessage($review.MainWindowHandle,0x84,[IntPtr]::Zero,$point).ToInt64() -ne $entry.expected){
            throw "Incorrect native titlebar hit region for $($entry.name)"
        }
    }
}
function Preferences {
    Invoke 'Preferences' -Name
    Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
}
function Close-Preferences($Dialog){
    (Control 'Close' -Name -Within $Dialog -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Preferences did not release canvas'
}
try{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/editor-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned editor review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyEditorKeys]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Set-Viewport
    Wait-Until {@((Model).panel_measurements|Where-Object {$_.panel -eq 'brushes' -and $_.content_height -gt 0 -and $_.content_height -ne 320}).Count -eq 1} 'Native content measurements did not reach Core'
    Start-Sleep -Milliseconds 400
    $model=Model
    foreach($panel in @('toolbar','brushes','tool_settings','color','navigator','properties','layers','commands')){
        if(@($model.layout.groups|Where-Object active -eq $panel).Count -ne 1){throw "Default editor does not show $panel"}
    }
    if(Find 'Drawing tool' -Name){throw 'Temporary tool chooser survived the full toolbar'}
    Check-Editor
    Check-Header
    Invoke 'application-menu-view';Invoke 'fit_canvas'
    Wait-Until {
        foreach($layer in (Model).state.layers){
            $thumbnail=Find "layer-$($layer.id)-thumbnail"
            if(!$thumbnail -or $thumbnail.Current.ItemStatus -ne 'Ready'){return $false}
        }
        $true
    } 'Initial layer thumbnails did not reach the native controls' 20
    (Control 'Drawing canvas' -Name).SetFocus()
    Start-Sleep -Milliseconds 350
    Capture 'editor-dark'
    [IO.File]::WriteAllText((Join-Path $run 'editor-model.json'),($model|ConvertTo-Json -Depth 100))
    $measured=$model.panel_measurements|ConvertTo-Json -Compress
    Start-Sleep -Milliseconds 600
    if(((Model).panel_measurements|ConvertTo-Json -Compress) -ne $measured){throw 'Native measurements did not settle'}
    & (Join-Path $PSScriptRoot 'exercise-tools.ps1') -ProcessId $review.Id -StateFile (Join-Path $directory 'ui-state.json')
    $normal=(Model).layout|ConvertTo-Json -Compress -Depth 70
    Set-Zen
    Wait-Until {(Model).partial_zen -and $null -ne (Find 'zen-tile-toolbar-1')} 'Native Zen toolbar did not appear'
    Start-Sleep -Milliseconds 300
    Check-Zen
    Select-Command 'brush' -Zen
    Wait-Until {(Command 'brush').selected} 'Zen tool did not reach shared state'
    $tileId=((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'}|Select-Object -ExpandProperty id
    Invoke "zen-tile-toolbar-$tileId"
    Wait-Until {$null -ne (Model).state.customization.drawer -and $null -ne (Find 'tool-drawer')} 'Zen color tile did not open drawer'
    $drawer=Control 'tool-drawer'
    Wait-Until {try{($drawer.Current.ItemStatus|ConvertFrom-Json).placement.bounds.width -eq 280}catch{$false}} 'Zen drawer did not reach shared geometry'
    Capture 'zen-color'
    Invoke "zen-tile-toolbar-$tileId"
    Wait-Until {$null -eq (Model).state.customization.drawer -and $null -eq (Find 'tool-drawer')} 'Zen color tile did not close drawer'
    Capture 'partial-zen'
    $retained=(Control 'zen-tile-toolbar-1').GetRuntimeId() -join ':'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 720
    Start-Sleep -Milliseconds 400
    Check-Zen
    if(((Control 'zen-tile-toolbar-1').GetRuntimeId() -join ':') -ne $retained){throw 'Zen resize replaced native tile controls'}
    Capture 'zen-narrow'
    Set-Viewport
    Set-Zen
    Wait-Until {!(Model).chrome_hidden -and $null -eq (Find 'zen-tile-toolbar-1')} 'Leaving Zen did not restore workspace'
    Start-Sleep -Milliseconds 400
    if(((Model).layout|ConvertTo-Json -Compress -Depth 70) -ne $normal){throw 'Zen changed saved workspace geometry'}
    Check-Editor
    $dialog=Preferences
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Light' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq 'light'} 'Light theme did not apply'
    Close-Preferences $dialog
    Start-Sleep -Milliseconds 400
    Check-Editor
    Check-Header
    Capture 'editor-light'
    Set-Zen
    Wait-Until {(Model).partial_zen} 'Light Zen did not activate'
    Start-Sleep -Milliseconds 300
    Check-Zen
    Capture 'zen-light'
    Set-Zen
    $dialog=Preferences
    (Control 'Total zen' -Name -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Close-Preferences $dialog
    Set-Zen
    Wait-Until {(Model).chrome_hidden -and !(Model).partial_zen -and $null -eq (Find 'zen-tile-toolbar-1')} 'Total Zen retained native toolbars'
    Set-Zen
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{full_editor='passed';titlebar_hit_regions='passed';core_rectangles='passed';native_measurements='passed';tools='passed';partial_zen='passed';zen_activation='passed';zen_drawer='passed';retained_resize='passed';restored_workspace='passed';themes='passed';total_zen='passed';zero_exit='passed';scope='native projection; complete visual and physical input acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
