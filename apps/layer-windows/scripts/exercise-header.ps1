param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('paint','sketch','photo')][string]$Workspace='paint',[switch]$Catalog)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition '
using System;
using System.Runtime.InteropServices;
public static class CapyStackCoordinates {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
}'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/header/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {
        if(!$script:statePath){
            foreach($path in [IO.Directory]::EnumerateFiles($directory,('ui-state-'+$review.Id+'-*.json'))){
                if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
                $value=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$script:statePath=$path;break}
            }
        }
        if($script:statePath){$value=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id){return $value.model}}
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Owned header review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Id,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id,[switch]$Name){(Control $Id -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}

function Header { (Model).header.model }
function HeaderJson { Header | ConvertTo-Json -Depth 30 -Compress }
function Presentation { try { (Control 'title-bar').Current.ItemStatus | ConvertFrom-Json } catch {} }
function Gesture { try { (Control 'title-bar').Current.HelpText | ConvertFrom-Json } catch {} }
function At([string]$Id) {
    $box=(Control $Id).Current.BoundingRectangle
    if($box.IsEmpty -or $box.Width -le 0 -or $box.Height -le 0){throw "Unarranged control: $Id"}
    @{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}
}
function Screen($Box) {
    $point=[CapyStackCoordinates+Point]::new()
    if(![CapyStackCoordinates]::ClientToScreen($review.MainWindowHandle,[ref]$point)){throw 'Client origin unavailable'}
    $scale=[CapyStackCoordinates]::GetDpiForWindow($review.MainWindowHandle)/96.
    @{x=[int]($point.x+$Box.x*$scale);y=[int]($point.y+$Box.y*$scale)}
}
function Tap([string]$Id) {
    $at=At $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y)
    Wait-Until {(Gesture).phase -eq 'pressed' -and (Gesture).source.value -eq [int]($Id.Split('-')[-1])} 'Native header did not receive the tap on the requested item'
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Native header did not finish the tap'
}
function WindowCommand([string]$Id) {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    Invoke $Id
}
function Capture([string]$Name) {
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Check-Geometry {
    Wait-Until {
        $p=Presentation
        if(!$p -or !$p.geometry.items.Count){return $false}
        foreach($item in $p.geometry.items){
            $actual=$p.actual_items|Where-Object id -eq $item.id|Select-Object -First 1
            if(!$actual){return $false}
            foreach($axis in @('x','y','width','height')){
                if([Math]::Abs($actual.bounds.$axis-$item.bounds.$axis) -gt 1){return $false}
            }
        }
        return $true
    } 'Native header does not match shared geometry'
}
function Start-Review([string]$Phase) {
    $script:statePath=$null;$script:stderr=Join-Path $run ($Phase+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/header-review.json')
    Write-Output "Owned header review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Header review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if($Phase -eq 'initial'){
        $choice=(Model).windows_workspace.switcher|Where-Object key -eq $Workspace|Select-Object -First 1
        if(!$choice){throw 'Requested workspace is missing'}
        if((Model).windows_workspace.id -ne $choice.id){
            $toggle=Find ('workspace-switch-'+$Workspace)
            if(!$toggle -or $toggle.Current.IsOffscreen){
                Invoke 'header-workspace-menu'
                $toggle=Control $choice.name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
            }
            $toggle.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
            Wait-Until {(Model).windows_workspace.id -eq $choice.id -and !(Model).windows_workspace.busy} 'Requested workspace did not open'
        }
        $bands=@((Model).state.workspace.layout.bands)
        if($Workspace -eq 'sketch' -and ((Header).size -ne 'medium' -or $bands.Count -ne 1 -or $bands[0].edge -ne 'left' -or $bands[0].alignment -ne 'center' -or (Model).state.workspace.layout.canvas_info.visible)){
            throw 'Sketch did not open the shared minimal titlebar layout'
        }
    }
    Check-Geometry
}
function Edit-Header {
    WindowCommand 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Titlebar editor did not open'
    Check-Geometry
    $null=Control 'header-edit-done'
}
function Drop-Component([string]$Kind='space',[switch]$Cancel,[switch]$Picker) {
    $before=HeaderJson;$from=At ('header-component-'+$Kind)
    $zone=(Presentation).geometry.zones[1]
    $to=Screen @{x=$zone.x+$zone.width*.5;y=$zone.y+$zone.height*.5}
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    [CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {(Gesture).phase -eq 'dragging' -and $null -ne (Gesture).preview.target} 'Immediate component drag has no shared target'
    if((HeaderJson) -ne $before){throw 'Drag preview changed the workspace header'}
    if($Cancel){[CapyRowPointer]::Key(0x1b)}
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Header capture did not retire'
    if($Cancel){if((HeaderJson) -ne $before){throw 'Escape committed the component'}}
    elseif($Picker){Wait-Until {(Model).picker} 'Tool drop did not open the existing picker';if((HeaderJson) -ne $before){throw 'Opening the picker saved a placeholder'}}
    else{Wait-Until {(HeaderJson) -ne $before} 'Valid component drop did not insert';Check-Geometry}
}


function Check-ItemDrag([int]$Id) {
    $before=HeaderJson
    $box=(Presentation).geometry.items|Where-Object id -eq $Id|Select-Object -ExpandProperty bounds
    $from=At ('header-select-'+$Id)
    $scale=[CapyStackCoordinates]::GetDpiForWindow($review.MainWindowHandle)/96.
    $distance=[int][Math]::Round($box.height*3*$scale)
    $far=@{x=$from.x;y=$from.y+$distance}
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    Wait-Until {(Gesture).phase -eq 'pressed' -and (Gesture).source.value -eq $Id} 'Header item did not receive the press'
    Start-Sleep -Milliseconds 900
    if($Device -eq 'mouse'){
        if((Gesture).menu_open -or (Gesture).phase -ne 'pressed'){throw 'Mouse hold opened a menu or started dragging'}
    }else{Wait-Until {(Gesture).phase -eq 'held' -and (Gesture).menu_open} 'Pen/touch hold did not open the item menu'}
    [CapyRowPointer]::Move($far.x,$far.y)
    Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).preview.detached -and !(Gesture).menu_open} 'Held item did not detach and close its menu'
    $held=(Gesture).preview.held
    if([Math]::Abs($held.x-$box.x) -gt 1 -or [Math]::Abs($held.y-($box.y+$distance/$scale)) -gt 1){throw 'Detached item lost its original grab offset'}
    if((HeaderJson) -ne $before){throw 'Detached preview changed the workspace'}
    [CapyRowPointer]::Move($from.x,$from.y)
    Wait-Until {(Gesture).preview.target -and !(Gesture).preview.detached} 'Returning item did not reattach'
    [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Item drag retained capture after Escape'
    if((HeaderJson) -ne $before){throw 'Cancelled item drag changed the header'}
    # Select through native keyboard activation before testing keyboard moves.
    Invoke ('header-select-'+$Id)
    foreach($step in 1..2){
        $previous=HeaderJson;[CapyRowPointer]::Key(0x27)
        Wait-Until {(HeaderJson) -ne $previous} 'Right key did not move the selected item'
    }
    if(!(@((Header).zones[2]|Where-Object id -eq $Id).Count)){throw 'Keyboard movement did not cross the zone boundary'}
}
function Check-ResizeCancel {
    $before=HeaderJson;$from=At 'header-component-space'
    $zone=(Presentation).geometry.zones[1]
    $to=Screen @{x=$zone.x+$zone.width*.5;y=$zone.y+$zone.height*.5}
    [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {(Gesture).preview.target} 'Resize case has no active placement preview'
    $size=$root.Current.BoundingRectangle
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width ([int]$size.Width+30) -Height ([int]$size.Height)
    Wait-Until {(Gesture).phase -eq 'idle'} 'Window resize did not cancel header capture'
    [CapyRowPointer]::Cancel()
    if((HeaderJson) -ne $before){throw 'Window resize committed a drag'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width ([int]$size.Width) -Height ([int]$size.Height)
    Check-Geometry
}
function Check-Fullscreen {
    foreach($active in @($true,$false)){
        $at=At 'header-item-1'
        [CapyRowPointer]::KeyAt(0x7a,$at.x,$at.y)
        Wait-Until {(Model).state.fullscreen -eq $active} 'Native F11 did not report the actual fullscreen state'
        Check-Geometry
        $clock=Entries|Where-Object {$_.item.kind -eq 'clock'}|Select-Object -First 1
        $visible=@((Presentation).geometry.items|Where-Object id -eq $clock.id).Count -gt 0
        if($visible -ne $active){throw 'Clock did not follow fullscreen visibility'}
        if((Model).header.editing){throw 'Fullscreen opened titlebar editing'}
    }
}

function Entries { @((Header).zones|ForEach-Object {$_}) }
function Picker-Search([string]$Query) {
    (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Query)
    Wait-Until {(Model).picker.query -eq $Query} 'Tool search did not reach shared state'
}
function Picker-Button([string]$Name) {
    (Control $Name -Name -Within (Control 'tool-picker') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Add-Tools {
    $before=HeaderJson;$firstId=(Header).next_id
    Drop-Component -Kind tools -Picker
    Picker-Button 'Cancel'
    Wait-Until {!((Model).picker) -and (Model).header.editing -and !(Find 'tool-picker') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Picker cancellation left the titlebar editor'
    if((HeaderJson) -ne $before){throw 'Picker Cancel changed the header'}
    Drop-Component -Kind tools -Picker
    foreach($tool in @(@('choose current paint color','picker-choice-color-0'),@('Brush','picker-choice-command-brush'))){
        Picker-Search $tool[0]
        (Control $tool[1]).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    }
    Picker-Search 'no-such-capy-tool-92831'
    Wait-Until {!((Model).picker.choices.Count)} 'Empty tool search retained choices'
    Picker-Search ''
    Wait-Until {(Model).picker.can_confirm} 'Search lost tool selection'
    Picker-Button 'Add Tools'
    Wait-Until {!((Model).picker) -and (Header).next_id -eq $firstId+2 -and !(Find 'tool-picker') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Picker did not add exactly two tools'
    $added=@(Entries|Where-Object id -ge $firstId)
    if($added[0].item.control.kind -ne 'color' -or $added[1].item.control.command -ne 'brush'){throw 'Tool insertion order changed across search'}
    Check-Geometry
    return $added
}
function Check-Drawer($Tools) {
    foreach($tool in $Tools){
        $frame=Control ('header-item-'+$tool.id)
        $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)
        $button=$frame.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
        $outer=$frame.Current.BoundingRectangle;$inner=$button.Current.BoundingRectangle
        if([Math]::Abs($outer.Width-$inner.Width) -gt 1 -or [Math]::Abs($outer.Height-$inner.Height) -gt 1){throw 'A native header tool does not fill its tile hit area'}
    }
    $color=$Tools[0].id;$brush=$Tools[1].id
    foreach($id in @($color,$brush,$brush)){
        $wasOpen=(Model).state.customization.drawer.anchor.id -eq $id
        $at=At ('header-item-'+$id)
        [CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
        if($wasOpen){
            Wait-Until {!((Model).state.customization.drawer)} 'Current header tool did not toggle its drawer closed'
        }else{
            Wait-Until {(Model).state.customization.drawer.anchor.kind -eq 'header' -and (Model).state.customization.drawer.anchor.id -eq $id} 'Header tool did not open or switch its drawer'
            $null=Control 'tool-drawer'
        }
    }
}


function Select-HeaderItem([int]$Id) {
    $entry=Find ('header-select-'+$Id)
    if($entry -and !$entry.Current.IsOffscreen){Invoke ('header-select-'+$Id);return}
    $p=Presentation
    foreach($zone in 0..2){
        if($Id -in $p.geometry.hidden[$zone]){
            Invoke ('header-overflow-'+$zone);Invoke ('header-overflow-item-'+$Id)
            Wait-Until {!(Find ('header-overflow-item-'+$Id))} 'Hidden item selection left its menu open'
            return
        }
    }
    throw "Header item $Id has no visible selection route"
}
function Remove-HeaderItem([int]$Id) {
    Select-HeaderItem $Id
    $at=At 'header-edit-done';[CapyRowPointer]::KeyAt(0x2e,$at.x,$at.y)
    Wait-Until {!(@(Entries|Where-Object id -eq $Id).Count)} "Header item $Id was not removed"
}
function Check-Catalog {
    $initial=HeaderJson
    Edit-Header
    foreach($component in (Model).header.components){
        $kind=$component.item.kind
        if($kind -eq 'fullscreen'){throw 'Web-only fullscreen appeared in the native component bank'}
        $existing=@(Entries|Where-Object {$_.item.kind -eq $kind}).Count -gt 0
        if($component.singleton -and $existing){
            if(Find ('header-component-'+$kind)){throw "Present singleton $kind remained in the bank"}
        }else{
            Drop-Component -Kind $kind
            if($component.singleton -and (Find ('header-component-'+$kind))){throw "Added singleton $kind remained in the bank"}
        }
    }
    $settings=Entries|Where-Object {$_.item.kind -eq 'settings'}|Select-Object -First 1
    Remove-HeaderItem $settings.id
    $null=Control 'header-component-settings'
    Drop-Component -Kind settings
    Capture 'catalog'
    Invoke 'header-edit-cancel'
    Wait-Until {(HeaderJson) -eq $initial -and !(Model).header.editing} 'Catalog Cancel lost the starting layout'
    Edit-Header
    foreach($id in @(Entries|ForEach-Object {$_.id})){Remove-HeaderItem $id}
    Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing -and !(Entries).Count} 'Empty titlebar did not commit'
    $null=Control 'header-recovery-menu'
    WindowCommand 'undo_workspace'
    Wait-Until {(HeaderJson) -eq $initial} 'Recovery menu could not undo the empty titlebar'
    Check-Geometry
    & (Join-Path $PSScriptRoot 'exercise-header-settings.ps1') -ProcessId $review.Id -StateFile $script:statePath
    Check-Geometry
}

function Check-Keyboard {
 $initial=HeaderJson
 (Control 'header-select-1').SetFocus()
 $at=At 'header-select-1';[CapyRowPointer]::KeyAt(0x20,$at.x,$at.y)
 Wait-Until {(Control 'header-select-1').Current.HasKeyboardFocus} 'Space moved editor focus'
 $seen=[Collections.Generic.List[object]]::new()
 for($i=0;$i -lt 55;$i++){
   [CapyRowPointer]::KeyAt(0x09,$at.x,$at.y);Start-Sleep -Milliseconds 70
   $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
   if($focus.Current.ProcessId -ne $review.Id){throw 'Tab left the owned application'}
   $record=@{id=$focus.Current.AutomationId;name=$focus.Current.Name;parent=''}
   for($node=$focus;$node;$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)){
     if($node.Current.AutomationId -like 'header-item-*'){$record.parent=$node.Current.AutomationId;break}
     if($node.Current.NativeWindowHandle -eq $review.MainWindowHandle){break}
   }
   $seen.Add($record)
   if($record.parent -and $record.id -notlike 'header-select-*'){$seen|ConvertTo-Json|Set-Content (Join-Path $run 'tab-order.json');throw 'Tab focused an underlying active tool while editing'}
   if($record.id -eq 'header-edit-done'){break}
 }
 $seen|ConvertTo-Json|Set-Content (Join-Path $run 'tab-order.json')
 if($seen[-1].id -ne 'header-edit-done'){throw 'Tab could not reach Done'}
 if((HeaderJson) -ne $initial -or (Model).chrome_hidden){throw 'Keyboard selection activated a normal header tool'}

}
function Check-Narrow {
 $initial=HeaderJson
 $size=$root.Current.BoundingRectangle
 $scale=[CapyStackCoordinates]::GetDpiForWindow($review.MainWindowHandle)/96.
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width ([int](640*$scale)) -Height ([int](480*$scale))
 Check-Geometry
 foreach($i in 1..12){Drop-Component}
 foreach($sizeName in @('small','medium','large')){
   Invoke ('header-size-'+$sizeName)
   Wait-Until {(Header).size -eq $sizeName} 'Narrow header size did not change'
   Check-Geometry
   $p=Presentation;$hidden=$null;$zoneIndex=0
   foreach($zoneIndex in 0..2){if($p.geometry.hidden[$zoneIndex].Count){$hidden=$p.geometry.hidden[$zoneIndex][0];break}}
   if(!$hidden){throw 'Fixture did not produce overflow at the minimum window size'}
   $ids=@(Entries|ForEach-Object {$_.id})
   Invoke ('header-overflow-'+$zoneIndex)
   Invoke ('header-overflow-item-'+$hidden)
   Wait-Until {!(Find ('header-overflow-item-'+$hidden))} 'Overflow menu did not close after selecting an item'
   $at=At ('header-overflow-'+$zoneIndex);[CapyRowPointer]::KeyAt(0x2e,$at.x,$at.y)
   Wait-Until {!(@(Entries|Where-Object id -eq $hidden).Count)} 'Delete did not remove the selected hidden item'
   $remaining=@(Entries|ForEach-Object {$_.id})
   if($remaining.Count -ne $ids.Count-1 -or @($ids|Where-Object {$_ -ne $hidden -and $_ -notin $remaining}).Count){throw 'Overflow deletion lost a hidden neighbor'}
   $done=Control 'header-edit-done';$bounds=$done.Current.BoundingRectangle;$window=$root.Current.BoundingRectangle
   if($done.Current.IsOffscreen -or $bounds.Left -lt $window.Left -or $bounds.Right -gt $window.Right -or $bounds.Bottom -gt $window.Bottom){throw 'Done is unreachable at the minimum window size'}
   Capture ('narrow-'+$sizeName)
 }
 Invoke 'header-edit-cancel'
 Wait-Until {!(Model).header.editing -and (HeaderJson) -eq $initial} 'Narrow overflow Cancel lost the starting layout'
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width ([int]$size.Width) -Height ([int]$size.Height)
 Check-Geometry
}

try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    Start-Review 'initial';Capture 'normal'
    if($Catalog){Check-Catalog}
    $initial=HeaderJson;$footer=(Model).state.workspace.layout.canvas_info.visible
    Edit-Header;Check-Keyboard
    foreach($size in @(@('medium',60),@('large',72),@('small',48))){
        Invoke ('header-size-'+$size[0])
        Wait-Until {(Header).size -eq $size[0] -and (Presentation).height -eq $size[1]} 'Size did not update the native header'
        Check-Geometry
    }
    $before=HeaderJson;$at=At 'header-component-space'
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 900
    if((Gesture).menu_open){throw 'A component-bank hold opened a menu'}
    [CapyRowPointer]::Up()
    if((HeaderJson) -ne $before){throw 'A component-bank click added an item'}
    Drop-Component -Cancel
    Check-ResizeCancel
    Drop-Component
    $space=@((Header).zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'space'})|Select-Object -Last 1
    if(!$space){throw 'No inserted space item'}
    Check-ItemDrag $space.id
    Tap ('header-select-'+$space.id)
    [CapyRowPointer]::Key(0x2e)
    Wait-Until {(HeaderJson) -ne $before -and !(@((Header).zones|ForEach-Object {$_}|Where-Object id -eq $space.id).Count)} 'Delete did not remove selected header item'
    (Control 'header-show-footer').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).state.workspace.layout.canvas_info.visible -ne $footer} 'Footer preview did not change'
    Invoke 'header-edit-cancel'
    Wait-Until {!(Model).header.editing -and (HeaderJson) -eq $initial} 'Cancel did not restore the initial header'
    if((Model).state.workspace.layout.canvas_info.visible -ne $footer){throw 'Cancel changed footer visibility'}
    Edit-Header;Drop-Component;if(!(Entries|Where-Object {$_.item.kind -eq 'clock'})){Drop-Component -Kind clock};$tools=Add-Tools;Capture 'customized'
    Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing} 'Done did not close titlebar editing'
    $saved=HeaderJson
    Check-Geometry;Check-Drawer $tools;Check-Fullscreen
    WindowCommand 'undo_workspace'
    Wait-Until {(HeaderJson) -eq $initial} 'One workspace Undo did not restore the initial header'
    WindowCommand 'redo_workspace'
    Wait-Until {(HeaderJson) -eq $saved} 'One workspace Redo did not restore the edited header'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [CapyRowPointer]::Dispose()
    Start-Review 'restart'
    Wait-Until {(HeaderJson) -eq $saved -and !(Model).header.editing} 'Restart did not restore the completed header edit'
    Capture 'restarted'
    Edit-Header;Check-Narrow
    Edit-Header;Drop-Component
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    [CapyRowPointer]::Dispose()
    Start-Review 'unsaved-restart'
    Wait-Until {(HeaderJson) -eq $saved -and !(Model).header.editing} 'Closing without Done saved a titlebar preview'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    [pscustomobject]@{device=$Device;workspace=$Workspace;catalog=[bool]$Catalog;geometry='passed';sizes='passed';native_keyboard='passed';resize_cancel='passed';held_detach_reattach='passed';keyboard_zones='passed';footer_rollback='passed';fullscreen='passed';narrow_overflow='passed';unsaved_restart='passed';inert_bank='passed';immediate_component_drag='passed';preview_and_escape='passed';delete_and_cancel='passed';picker_search_and_order='passed';native_tile_hit_area='passed';drawer_switch_and_toggle='passed';done_undo_redo='passed';restart='passed';scope='OS-delivered synthetic input; physical devices and full visual acceptance are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){
        try{@{gesture=Gesture;header=Header;presentation=Presentation}|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run 'failure-state.json');Capture 'failure'}catch{}
        if($Device -eq 'mouse' -and [CapyRowPointer]::Active){try{[CapyRowPointer]::Key(0x1b)}catch{}}
    }
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
