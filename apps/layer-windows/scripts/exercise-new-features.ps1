param([Parameter(Mandatory)][string]$Executable,[ValidateSet('Pointer','Automation')][string]$InputMode='Pointer')
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
 [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr window);
 public static void Focus(IntPtr window) {SetForegroundWindow(window);}

}'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/new-features/'+[Guid]::NewGuid().ToString('N'))
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
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8,[switch]$Closing){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){if($Closing){return};throw 'Owned feature review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
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

function At([string]$Id) {
    $ready=@{box=$null}
    Wait-Until {$c=Find $Id;if(!$c -or $c.Current.IsOffscreen){return $false};$ready.box=$c.Current.BoundingRectangle;return $ready.box.Width -gt 0 -and $ready.box.Height -gt 0} "Unarranged $Id"
    @{x=[int]($ready.box.X+$ready.box.Width/2);y=[int]($ready.box.Y+$ready.box.Height/2)}
}
function Tap([string]$Id,[string]$Device='mouse') {
    if($Id -match '^header-item-(\d+)$'){
        $entry=[int]$Id.Substring('header-item-'.Length);$zone=-1
        for($i=0;$i -lt 3;$i++){if(@((Model).header.model.zones[$i]|Where-Object id -eq $entry).Count){$zone=$i;break}}
        Wait-Until {(Find $Id) -or (Find ('header-overflow-'+$zone))} "Header item $entry did not arrange"
        if(!(Find $Id)){Invoke ('header-overflow-'+$zone);$Id='header-overflow-item-'+$entry}
    }
    if($InputMode -eq 'Automation'){
        $control=Control $Id
        if(!$control.Current.IsKeyboardFocusable){$control=$control.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsKeyboardFocusableProperty,$true))}
        if(!$control){throw "No accessible button for $Id"}
        Wait-Until {$control.Current.IsEnabled} "Disabled $Id" 120
        $pattern=$null
        if($control.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
        elseif($control.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
        else{throw "No action for $Id"}
        Start-Sleep -Milliseconds 250;return
    }
    Wait-Until {(Control $Id).Current.IsEnabled -and (Model).brush_ready} "Disabled $Id" 120
    $null=[CapyStackCoordinates]::Focus($review.MainWindowHandle);Start-Sleep -Milliseconds 100
    $at=At $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 60;[CapyRowPointer]::Up();Start-Sleep -Milliseconds 240
}
function HeaderId([string]$Kind,[string]$Value) {
    $entry=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.$Kind -eq $Value})[0]
    if(!$entry){throw "Header lacks $Kind=$Value"};'header-item-'+$entry.id
}
function Drawer([string[]]$Panels) {
    Wait-Until {$d=(Model).state.customization.drawer; $d -and (@($d.columns|ForEach-Object {$_}) -join ',') -eq ($Panels -join ',')} "Wrong drawer: $Panels"
    foreach($panel in $Panels){$null=Control ('drawer-panel-'+$panel)}
}
function Capture([string]$Name) {
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
    (Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))
}
function Swipe([int]$Id,[int]$Dx,[string]$Device) {
    $at=At "layer-$Id-name";[CapyRowPointer]::Down($Device,$at.x,$at.y)
    for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move($at.x+[int]($Dx*$i/8),$at.y);Start-Sleep -Milliseconds 20}
    [CapyRowPointer]::Up();Start-Sleep -Milliseconds 200
}
function Preference([string]$Title){@((Model).preferences.pages.groups.rows|Where-Object title -eq $Title)[0]}
function Preference-Switch([string]$Title){Control $Title -Name -Type ([System.Windows.Automation.ControlType]::Button)}
function Check-InputPreferences {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
    Invoke 'Preferences' -Name;Invoke 'Pen & Input' -Name
    $expected=@('None','Cross','Triangle','Dot','Single-pixel dot','Sight','Brush size','Brush size and cross','Brush size and dot','Brush size and single-pixel dot')
    Wait-Until {$null -ne (Find 'Cursor shape' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox))} 'Cursor shape setting missing'
    if(((Preference 'Cursor shape').kind.options -join ',') -ne ($expected -join ',')){throw 'Cursor choices differ from shared model'}
    foreach($index in 0..9){
        (Control 'Cursor shape' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        (Control $expected[$index] -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Wait-Until {(Preference 'Cursor shape').kind.selected -eq $index} 'Cursor choice did not apply';Start-Sleep -Milliseconds 200
    }
    if(!(Preference 'Hide cursor when painting').kind.active){throw 'Drawing cursor hiding is not enabled by default'}
    (Preference-Switch 'Hide cursor when painting').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {!(Preference 'Hide cursor when painting').kind.active} 'Cursor hiding did not disable'
    (Preference-Switch 'Hide cursor when painting').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    if(!(Preference 'Use Windows stroke prediction').kind.active -or (Preference 'Prediction amount').enabled){throw 'Native prediction defaults differ'}
    (Preference-Switch 'Use Windows stroke prediction').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {!(Preference 'Use Windows stroke prediction').kind.active -and (Preference 'Prediction amount').enabled} 'Shared prediction fallback did not enable'
    (Preference-Switch 'Use Windows stroke prediction').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    (Control 'Cursor shape' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Brush size' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Capture 'input-preferences'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Close' -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Model).preferences} 'Input preferences did not close'
    Write-Output 'PASS: All ten cursor modes, painting cursor visibility, native/shared prediction preferences'
}
function Set-Theme([string]$Theme) {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
    Invoke 'Preferences' -Name
    Wait-Until {$null -ne (Find 'Color theme' -Name)} 'Preferences missing'
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Theme -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq $Theme.ToLowerInvariant()} 'Theme did not apply'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Close' -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find 'Color theme' -Name)} 'Preferences did not close'
    $null=[CapyStackCoordinates]::Focus($review.MainWindowHandle);Start-Sleep -Milliseconds 200
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned feature review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Feature review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));$null=[CapyStackCoordinates]::Focus($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Check-InputPreferences
    $switch=Find 'workspace-switch-sketch'
    if(!$switch -or $switch.Current.IsOffscreen){Invoke 'header-workspace-menu';$switch=Control 'Sketch' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:painter' -and !(Model).windows_workspace.busy} 'Sketch did not open'
    $brush=HeaderId 'command' 'drawing_brush';$sculpt=HeaderId 'command' 'sculpt';$eraser=HeaderId 'command' 'eraser'
    $filters=HeaderId 'panel' 'adjustments';$layers=HeaderId 'panel' 'layers'
    Tap $brush;Drawer @('brush_sets','tools','tool_settings')
    $sets=@((Model).state.tool_panels.brush_sets.groups)
    if(($sets.label -join ',') -ne 'Pen,Marker,Pencil,Pastel,Paint,Watercolor,Oil paint,Airbrush,Spray,Texture'){throw 'Brush categories differ from shared reference'}
    foreach($pair in @(@('mouse','pencil'),@('touch','pastel'),@('pen','paintbrush'))){
        $choice=$sets|Where-Object {$_.icon -eq $pair[1] -or $_.label -eq $(switch($pair[1]){'pencil'{'Pencil'};'pastel'{'Pastel'};default{'Paint'}})}|Select-Object -First 1
        Tap ('brush-set-'+$choice.icon) $pair[0];Drawer @('brush_sets','tools','tool_settings')
        $preset=(Model).state.tool_panels.tools.subtools[0].preview
        Tap 'tool-subtool-0' $pair[0];Wait-Until {(Model).state.brush.preset -eq $preset} 'Subtool did not activate'
    }
    $drawing=(Model).state.brush.preset
    foreach($theme in @('Light','Dark')){Set-Theme $theme;Tap $brush;if(!(Model).state.customization.drawer){Tap $brush};Drawer @('brush_sets','tools','tool_settings');Capture ('brush-'+$theme.ToLowerInvariant())}
    Tap $sculpt 'pen';Drawer @('sculpt_sets','tools','tool_settings')
    foreach($device in @('mouse','touch','pen')){foreach($choice in @((Model).state.tool_panels.sculpt_sets.groups)){Tap ('sculpt-set-'+$choice.icon) $device;Drawer @('sculpt_sets','tools','tool_settings')}}
    $sculptPreset=(Model).state.brush.preset;Capture 'sculpt-dark'
    Tap $eraser 'touch';Drawer @('tools','tool_settings');Capture 'eraser-dark'
    Tap $brush;Drawer @('brush_sets','tools','tool_settings');if((Model).state.brush.preset -ne $drawing){throw 'Drawing tool memory lost'}
    Tap $sculpt;Drawer @('sculpt_sets','tools','tool_settings');if((Model).state.brush.preset -ne $sculptPreset){throw 'Sculpt tool memory lost'}
    Write-Output "PASS: Brush/Sculpt/Eraser drawers, $InputMode input, retained tool memory"
    Tap $filters;Drawer @('filter_types','adjustments','properties')
    Wait-Until {@((Model).state.adjustments).Count -gt 1} 'Filters not loaded' 120
    $count=@((Model).state.layers).Count;$choices=@((Model).state.adjustments|Select-Object -First 2);$filterId=$null
    $i=0;foreach($device in @('mouse','touch','pen')){
        $choice=$choices[$i%2];Tap ('filter-'+$choice.id) $device
        Wait-Until {(Model).state.filter_picker.selected -eq $choice.id} 'Filter did not select'
        $current=(Model).state.layer_properties.layer
        if($null -ne $filterId -and $current -ne $filterId){throw 'Replacement changed filter identity'};$filterId=$current
        if(@((Model).state.layers).Count -ne $count+1){throw 'Replacement inserted another filter'}
        if((Control ('filter-'+$choice.id)).Current.ItemStatus -ne 'Selected'){throw 'Selected filter has no native highlight'}
        if(!(@((Model).state.layers|Where-Object drawing).Count)){throw 'Filter lost drawing target'};$i++
    }
    Tap $filters;Wait-Until {!(Model).state.customization.drawer} 'Filter drawer did not close'
    Tap $filters;Drawer @('filter_types','adjustments','properties');if((Model).state.layer_properties.layer -ne $filterId){throw 'Filter selection lost on reopen'}
    Capture 'filters-dark';Set-Theme 'Light';Tap $filters;Drawer @('filter_types','adjustments','properties');Capture 'filters-light'
    Tap 'cancel-filter' 'pen';Wait-Until {!(Model).state.customization.drawer -and @((Model).state.layers).Count -eq $count} 'Cancel did not delete filter and close'
    Tap $layers;$paper=@((Model).state.layers|Where-Object label -eq 'Paper')[0];Tap ('layer-'+$paper.id+'-name')
    Tap $filters;Drawer @('filter_types','adjustments','properties');Tap 'paper-color-bucket' 'touch';Capture 'paper-properties'
    if(!(Model).state.layer_properties.controls){throw 'Paper properties missing'}
    Write-Output 'PASS: Filter replacement/reopen/cancel, drawing target, paper color'
    Tap $layers;Drawer @('layers')
    Wait-Until {$image=Find ('layer-'+$paper.id+'-thumbnail');$image -and $image.Current.ItemStatus -eq 'Ready'} 'Paper thumbnail did not finish' 30
    Capture 'paper-layer'
    if($InputMode -eq 'Pointer'){
        foreach($device in @('pen','touch')){
            $id=@((Model).state.layers)[0].id;Swipe $id -90 $device
            Wait-Until {$button=Find "layer-$id-swipe-delete";$button -and !$button.Current.IsOffscreen -and $button.Current.BoundingRectangle.Width -gt 60} 'Swipe did not reveal Delete'
            Capture ('swipe-'+$device);Swipe $id 90 $device
            Wait-Until {$button=Find "layer-$id-swipe-delete";!$button -or $button.Current.IsOffscreen} 'Reverse swipe did not close'
            Swipe $id -90 $device;[CapyRowPointer]::Key(0x1b)
            Wait-Until {$button=Find "layer-$id-swipe-delete";!$button -or $button.Current.IsOffscreen} 'Escape did not close swipe'
            Swipe $id -90 $device;Tap 'layer-actions' $device
            Wait-Until {$button=Find "layer-$id-swipe-delete";!$button -or $button.Current.IsOffscreen} 'Outside press did not close swipe'
            [CapyRowPointer]::Key(0x1b)
            Swipe $id -90 $device;Tap "layer-$id-swipe-delete" $device
            Wait-Until {!(@((Model).state.layers|Where-Object id -eq $id).Count)} 'Swipe Delete did not remove layer'
        }
        if(@((Model).state.layers).Count -ne 0){throw 'Final layer was not deleted'};Capture 'empty-canvas'
        [CapyRowPointer]::Key(0x1b)
        foreach($step in 1..$count){
            & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
            Invoke 'Undo' -Name
            Wait-Until {@((Model).state.layers).Count -eq $step} 'Delete Undo did not restore layer'
        }
        Wait-Until {@((Model).state.layers).Count -eq $count} 'Delete Undo did not restore both layers'
        Write-Output 'PASS: Pen/touch swipe-delete, reversal/Escape/outside cancellation, empty document and Undo'
    }
    if($InputMode -eq 'Automation'){
        foreach($step in 1..$count){
            $id=@((Model).state.layers)[0].id;Tap ('layer-'+$id+'-name');Tap 'layer-delete'
            Wait-Until {!(@((Model).state.layers|Where-Object id -eq $id).Count)} 'Delete did not remove layer'
        }
        if(@((Model).state.layers).Count -ne 0){throw 'Final layer was not deleted'};Capture 'empty-canvas'
        foreach($step in 1..$count){
            & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
            Invoke 'Undo' -Name
            Wait-Until {@((Model).state.layers).Count -eq $step} 'Delete Undo did not restore layer'
        }
        Write-Output 'PASS: Accessible deletion of Paper and the last layer, empty canvas and Undo'
    }
    [CapyRowPointer]::Dispose()
    # Use this review's atomic snapshot; ui-state.json can be read mid-write.
    $null=$review.CloseMainWindow();$lastDecision=$null
    Wait-Until {
        $review.Refresh();if($review.HasExited){return $true}
        $m=Model;$discard=Find 'Discard Changes' -Name -Type ([System.Windows.Automation.ControlType]::Button)
        if($m.windows_isolated_settings -and $discard -and $discard.Current.IsEnabled -and $lastDecision -ne $m.state.document_file.epoch){
            $discard.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();$lastDecision=$m.state.document_file.epoch
        }
        return $false
    } 'Owned review did not close' 20 -Closing
    if($review.ExitCode -ne 0){throw "Native review exit code $($review.ExitCode)"}

    if((Get-Item $stderr).Length){throw 'Review stderr is not empty'}
    @{status='passed';captures=$run;input_mode=$InputMode;swipe_tested=($InputMode -eq 'Pointer');scope='Pointer mode uses guarded OS-delivered synthetic input; Automation mode checks accessible commands only. Physical digitizers are not tested.'}|ConvertTo-Json
} catch {
    $_|Out-String|Set-Content (Join-Path $run 'failure.txt');$_.ScriptStackTrace|Add-Content (Join-Path $run 'failure.txt')
    if($review){$review.Refresh();if(!$review.HasExited){try{Capture 'failure'}catch{$_|Out-String|Set-Content (Join-Path $run 'capture-error.txt')}}};throw
} finally {
    [CapyRowPointer]::Dispose()
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
