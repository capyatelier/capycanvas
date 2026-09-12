param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected an isolated CapyCanvas review process'}
function Model {
    try {$snapshot=Get-Content -LiteralPath $StateFile -Raw|ConvertFrom-Json;if($snapshot.process_id -eq $ProcessId){return $snapshot.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};Start-Sleep -Milliseconds 75}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review startup did not complete' 45
if(!(Model).windows_isolated_settings){throw 'Use an isolated review settings profile'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
function Find([string]$Value,$Type=[System.Windows.Automation.ControlType]::Button,[switch]$Id){
    $property=if($Id){[System.Windows.Automation.AutomationElement]::AutomationIdProperty}else{[System.Windows.Automation.AutomationElement]::NameProperty}
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new($property,$Value),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,$Type=[System.Windows.Automation.ControlType]::Button,[switch]$Id){
    $script:found=$null
    Wait-Until {$script:found=Find $Value $Type -Id:$Id;$null -ne $script:found} "Missing control: $Value"
    $script:found
}
function Invoke-Control([string]$Name){(Control $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Invoke-Id([string]$Id){(Control $Id -Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Field([string]$Id){Control ('tool-setting-'+$Id) ([System.Windows.Automation.ControlType]::Edit) -Id}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Focus($Entry){$Entry.SetFocus();Wait-Until {$Entry.Current.HasKeyboardFocus} 'Control did not receive focus'}
function Draft([string]$Id,[string]$Text){
    $entry=Field $Id;Focus $entry
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
    Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Text} 'Draft was not retained'
}
function Select-Tool([string]$Id){
    $target=$null
    foreach($panel in (Model).panels){
        $tile=$panel.tiles|Where-Object {$_.control.kind -eq 'command' -and $_.control.command -eq $Id}|Select-Object -First 1
        if($tile){$target="tile-$($panel.id)-$($tile.id)";break}
    }
    if(!$target){throw "Command has no native toolbar tile: $Id"}
    Invoke-Id $target
    Wait-Until {
        if($Id -eq 'scale_rotate'){return (Model).state.layer_tools.tool -eq 'transform'}
        @((Model).state.commands|Where-Object {$_.id -eq $Id -and $_.selected}).Count -eq 1
    } "Tool did not activate: $Id"
}
function Check-Projection {
    $state=(Model).state
    foreach($set in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){
        $items=@($state.tool_set.($set[0]))
        for($i=0;$i -lt $items.Count;$i++){
            $item=$items[$i];$native=Control ($set[1]+$i) -Id
            if($native.Current.Name -ne $item.label){throw 'Tool label differs from shared model'}
            Wait-Until {($native.Current.ItemStatus -eq 'Selected') -eq $item.selected} 'Tool selection decoration differs from shared model'
        }
        if(Find ($set[1]+$items.Count) -Id){throw 'Stale tool controls survived a schema change'}
    }
    foreach($setting in $state.tool_settings){
        if((Field $setting.id).Current.Name -ne $setting.label){throw 'Tool setting label differs from shared model'}
    }
    foreach($action in $state.tool_actions){
        $command=$state.commands|Where-Object id -eq $action.command
        $type=if($action.checkable){[System.Windows.Automation.ControlType]::CheckBox}else{[System.Windows.Automation.ControlType]::Button}
        $native=Control ('tool-action-'+$action.command) $type -Id
        if($native.Current.Name -ne $command.label -or $native.Current.IsEnabled -ne $command.enabled){throw 'Tool command label/enabled state differs'}
        if($action.checkable){
            $checked=$native.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
            if($checked -ne $command.selected){throw 'Tool checkbox differs from shared state'}
        }
    }
}
if(!@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    (Control 'Tool panel' ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count -gt 0} 'Tool panel did not open'
}
if(!@((Model).layout.groups|Where-Object active -eq 'tool_settings').Count){Invoke-Id 'panel-tab-tool_settings'}
Wait-Until {@((Model).layout.groups|Where-Object active -eq 'tool_settings').Count -eq 1} 'Tool panel did not activate'
$null=Field 'flow'
Select-Tool 'pen'
$entry=Field 'flow';$original=$entry.GetRuntimeId() -join ':'
$subtool=(Control 'tool-subtool-0' -Id).GetRuntimeId() -join ':'
Draft 'flow' '40 + 2'
Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-.42) -lt .0001} 'Flow expression was not committed through the shared core'
if(((Field 'flow').GetRuntimeId() -join ':') -ne $original){throw 'A value update replaced the tool field'}
if(((Control 'tool-subtool-0' -Id).GetRuntimeId() -join ':') -ne $subtool){throw 'A value update replaced the subtool button'}
# Native focus can leave the field before an accessibility invocation runs.
# That may commit to the old brush; the detached draft must never overwrite
# the newly selected brush, including when focus moves again afterwards.
Invoke-Id 'tool-group-1'
Wait-Until {(Model).state.tool_set.groups[1].selected} 'Marker group did not activate'
$markerFlow=Value 'flow'
Invoke-Id 'tool-group-0'
Wait-Until {(Model).state.tool_set.groups[0].selected} 'Pen group did not restore'
Draft 'flow' '77'
Invoke-Id 'tool-group-1'
Wait-Until {(Model).state.tool_set.groups[1].selected} 'Marker group did not activate'
Check-Projection
Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-$markerFlow) -lt .0001} 'Detached draft overwrote the new brush'
Invoke-Id 'tool-group-0'
Wait-Until {(Model).state.tool_set.groups[0].selected} 'Pen group did not restore'
if(((Field 'flow').GetRuntimeId() -join ':') -eq $original){throw 'Changing tools retained an old draft context'}
# Target changes must discard drafts even when the numeric schema is identical.
Draft 'flow' '88'
$oldTarget=(Model).state.layer_tools.editing_layer.id
$oldField=(Field 'flow').GetRuntimeId() -join ':'
Invoke-Control 'New layer'
Wait-Until {(Model).state.layer_tools.editing_layer.id -ne $oldTarget} 'Layer target did not change'
Wait-Until {((Field 'flow').GetRuntimeId() -join ':') -ne $oldField} 'Layer change retained the old field context'
# Moving focus to a layer command can commit to the old context before the
# command runs. Once the target changes, the new field must show acknowledged
# state and cannot replay an old draft on a later focus change.
$acknowledged=Value 'flow'
$display=(Field 'flow').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
if($display -notmatch ('^'+[Math]::Round($acknowledged*100))){throw 'New target retained an unacknowledged draft'}
Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-$acknowledged) -lt .0001} 'Detached tool field overwrote the new target'
Invoke-Control 'Undo'
# All catalog tools must be reachable and project the actual shared schema.
$tools=@('pen','pencil','brush','eraser','airbrush','decoration','blend','liquify','lasso','move','hand','eyedropper','gradient','figure','ruler','auto_select','fill')
foreach($tool in $tools){Select-Tool $tool;Check-Projection}
# A long brush schema must retain its actual native scroll container and offset
# when a visible slider changes a value. An offscreen UIA slider intentionally
# scrolls into view, so it cannot test this retention invariant.
Select-Tool 'brush'
Invoke-Id 'tool-group-1'
Wait-Until {(Model).state.tool_set.groups[1].selected} 'Watercolor group did not activate'
Check-Projection
$cursor=Field 'flow';$scroll=$null;$walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker
while($cursor){
    $pattern=$null
    if($cursor.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern) -and $pattern.Current.VerticallyScrollable){$scroll=$pattern;break}
    $cursor=$walker.GetParent($cursor)
}
if(!$scroll){throw 'Long tool settings did not expose native scrolling'}
$slider=Control 'tool-setting-water_load-slider' ([System.Windows.Automation.ControlType]::Slider) -Id
Focus $slider
$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,100)
Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-100) -lt .01} 'Tool panel did not finish scrolling'
$scrollIdentity=$cursor.GetRuntimeId() -join ':'
$slider=Control 'tool-setting-water_load-slider' ([System.Windows.Automation.ControlType]::Slider) -Id
$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.35)
Wait-Until {[Math]::Abs((Value 'water_load')-.35) -lt .0001} 'Tool slider did not update shared water load'
if(($cursor.GetRuntimeId() -join ':') -ne $scrollIdentity -or [Math]::Abs($scroll.Current.VerticalScrollPercent-100) -gt .01){throw 'Tool edit replaced or reset scrolling'}
Select-Tool 'gradient'
Invoke-Id 'tool-subtool-3'
Wait-Until {(Model).state.tool_set.subtools[3].selected} 'Radial transparent gradient did not select'
Check-Projection
Select-Tool 'figure'
Invoke-Id 'tool-group-1'
Wait-Until {(Model).state.tool_set.groups[1].selected} 'Rectangle did not select'
Invoke-Id 'tool-subtool-1'
Wait-Until {(Model).state.tool_set.subtools[1].selected} 'Filled rectangle did not select'
Check-Projection
if(Find 'tool-setting-size' ([System.Windows.Automation.ControlType]::Edit) -Id){throw 'Fill retained an irrelevant line-width field'}
Select-Tool 'ruler'
$check=Control 'tool-action-show_rulers' ([System.Windows.Automation.ControlType]::CheckBox) -Id
$before=((Model).state.commands|Where-Object id -eq 'show_rulers').selected
$check.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Wait-Until {((Model).state.commands|Where-Object id -eq 'show_rulers').selected -ne $before} 'Ruler toggle did not reach core'
Check-Projection
$check.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
Select-Tool 'pen'
Invoke-Control 'Test stroke'
Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq 'undo' -and $_.enabled}).Count -eq 1} 'Controlled stroke did not complete'
Select-Tool 'scale_rotate'
Check-Projection
Invoke-Id 'tool-action-cancel_transform'
Wait-Until {@((Model).state.tool_actions|Where-Object command -eq 'cancel_transform').Count -eq 0} 'Transform cancel did not finish'
Select-Tool 'pen'
[pscustomobject]@{tool_and_subtool_projection='passed';shared_numeric_expression='passed';retained_fields_and_buttons='passed';retained_scrolling='passed';
    stale_tool_draft='passed';target_context_replacement='passed';figure_schema_change='passed';gradient_subtools='passed';ruler_toggle='passed';transform_cancel='passed';
    scope='native UI Automation and shared state; physical input and full-editor visual parity remain open'}|ConvertTo-Json
