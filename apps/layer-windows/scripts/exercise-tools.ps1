param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile,[string]$OutputDirectory)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected an isolated CapyCanvas review process'}
if(!$OutputDirectory){$OutputDirectory=Split-Path -Parent $StateFile}
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
function Model {
    $snapshot=Read-Snapshot $StateFile
    if($snapshot.process_id -ne $ProcessId){throw 'Review snapshot process owner mismatch'}
    return $snapshot.model
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{try{if(& $Condition){return}}catch{};Start-Sleep -Milliseconds 75}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
$native=@{window=$null}
Wait-Until {$native.window=Owned-DrawingWindow $app;$null -ne $native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Review startup did not complete' 45
if(!(Model).windows_isolated_settings){throw 'Use an isolated review settings profile'}
$root=$native.window.Root;$drawingWindow=$native.window.Handle
function Find([string]$Value,$Type=[System.Windows.Automation.ControlType]::Button,[switch]$Id){
    $property=if($Id){[System.Windows.Automation.AutomationElement]::AutomationIdProperty}else{[System.Windows.Automation.AutomationElement]::NameProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,$Type=[System.Windows.Automation.ControlType]::Button,[switch]$Id){
    $script:found=$null
    Wait-Until {$script:found=Find $Value $Type -Id:$Id;$null -ne $script:found} "Missing control: $Value"
    $script:found
}
function Arranged([string]$Id,$Type){
    $hit=@{item=$null;bounds=$null;stable=0}
    Wait-Until {
        $hit.item=Find $Id $Type -Id
        if(!$hit.item -or $hit.item.Current.IsOffscreen){return $false}
        $bounds=$hit.item.Current.BoundingRectangle
        if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
        if($bounds -eq $hit.bounds){$hit.stable++}else{$hit.stable=0;$hit.bounds=$bounds}
        $hit.stable -ge 2
    } "Tool control did not arrange: $Id"
    $hit.item
}
function Reveal-Control([string]$Id,[string]$RootId,$Type,[double]$Height,[double]$Scale){
    $entry=Control $Id $Type -Id
    $entry.SetFocus()
    Wait-Until {$entry.Current.HasKeyboardFocus} "Tool control did not receive focus: $Id"
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker
    $node=$walker.GetParent($entry);$scroll=$null;$viewport=$null
    while($node -and !$scroll){
        $pattern=$null
        if($node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$pattern) -and $pattern.Current.VerticallyScrollable){$scroll=$pattern;$viewport=$node}
        $node=$walker.GetParent($node)
    }
    $panel=@((Model).layout.groups|Where-Object active -eq 'tool_settings')
    if($panel.Count -ne 1){throw 'Tool panel does not have one active native group'}
    $panelBounds=Native-Bounds $panel[0].id $Scale -Group
    for($step=0;$step -lt 20;$step++){
        $field=Control $RootId $null -Id;$bounds=$field.Current.BoundingRectangle
        $clip=if($viewport){$viewport.Current.BoundingRectangle}else{$root.Current.BoundingRectangle}
        $clip.Intersect($panelBounds)
        $clip.Inflate(1,1)
        if(!$field.Current.IsOffscreen -and $clip.Contains($bounds) -and $bounds.Height/$Scale -ge $Height-1){
            return (Arranged $RootId $null)
        }
        if(!$scroll){throw "Partially visible control has no scroll container: $Id"}
        $navigationBounds=Native-Bounds $RootId $Scale
        $below=$navigationBounds.Top -ge ($clip.Top+$clip.Bottom)/2
        $direction=if($navigationBounds.Top -ge $clip.Bottom){[System.Windows.Automation.ScrollAmount]::LargeIncrement}elseif($navigationBounds.Bottom -le $clip.Top){[System.Windows.Automation.ScrollAmount]::LargeDecrement}elseif($below){[System.Windows.Automation.ScrollAmount]::SmallIncrement}else{[System.Windows.Automation.ScrollAmount]::SmallDecrement}
        $before=$scroll.Current.VerticalScrollPercent
        try{
            $scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,$direction)
            Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-$before) -gt .01} "Control $Id did not scroll fully into view"
        }catch{
            try{
                $revealFailureRecord=[ordered]@{step=$step;id=$Id;root_id=$RootId;expected_height=$Height;scale=$Scale;field_bounds=$bounds;clip=$clip;panel_bounds=$panelBounds;navigation_bounds=$navigationBounds;direction=[string]$direction;before_percent=$before}
                try{$failedViewport=$viewport.Current;$revealFailureRecord.viewport=@{automation_id=$failedViewport.AutomationId;class_name=$failedViewport.ClassName;runtime_id=@($viewport.GetRuntimeId())}}catch{$revealFailureRecord.viewport_error=$_.Exception.Message}
                try{$failedScroll=$scroll.Current;$revealFailureRecord.scroll_current=@{percent=$failedScroll.VerticalScrollPercent;view_size=$failedScroll.VerticalViewSize;vertically_scrollable=$failedScroll.VerticallyScrollable}}catch{$revealFailureRecord.scroll_error=$_.Exception.Message}
                try{$failedField=$field.Current;$revealFailureRecord.field_current=@{bounds=$failedField.BoundingRectangle;offscreen=$failedField.IsOffscreen}}catch{$revealFailureRecord.field_error=$_.Exception.Message}
                try{$failedWorkspace=Workspace-Root;$revealFailureRecord.native_publication=$failedWorkspace.Current.ItemStatus}catch{$revealFailureRecord.native_publication_error=$_.Exception.Message}
                try{$failedOrigin=[CapyRowPointer+Point]::new();$failedHandle=[IntPtr]$root.Current.NativeWindowHandle;$revealFailureRecord.client_origin=@{native_window_handle=$failedHandle.ToInt64();available=[CapyRowPointer]::ClientToScreen($failedHandle,[ref]$failedOrigin);point=$failedOrigin}}catch{$revealFailureRecord.client_origin_error=$_.Exception.Message}
                $revealFailureRecord|ConvertTo-Json -Depth 8|Set-Content -LiteralPath (Join-Path $OutputDirectory 'reveal-control-scroll-failure.json') -Encoding utf8
            }catch{}
            throw
        }
    }
    throw "Control $Id remained clipped after scrolling"
}
function Reveal-Number([string]$Id,[double]$Height,[double]$Scale){Reveal-Control ('tool-setting-'+$Id) ('number-root-tool-setting-'+$Id) ([System.Windows.Automation.ControlType]::Edit) $Height $Scale}
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
function Select-Preset([int]$Preset){
    $items=@((Model).state.tool_set.subtools);$indices=@(for($i=0;$i -lt $items.Count;$i++){if($items[$i].action.type -eq 'select_brush' -and $items[$i].action.id -eq $Preset){$i}})
    if($indices.Count -ne 1){throw "Preset $Preset has no unique native Tool Set choice"}
    Invoke-Id ('tool-subtool-'+$indices[0])
    Wait-Until {(Model).state.brush.preset -eq $Preset -and (Model).brush_ready} "Preset $Preset did not activate"
}
function Select-Tool([string]$Id,[switch]$Cancel){
    $target=@{id=$null}
    Wait-Until {
        foreach($panel in (Model).panels){
            $tile=$panel.tiles|Where-Object {$_.control.command -eq $Id -or $_.resolved_control.command -eq $Id}|Select-Object -First 1
            if($tile){$target.id="tile-$($panel.id)-$($tile.id)";return $true}
        }
        $false
    } "Command has no native toolbar tile: $Id"
    Invoke-Id $target.id
    if($Cancel){return}
    Wait-Until {
        if($Id -eq 'scale_rotate'){return (Model).state.layer_tools.tool -eq 'transform'}
        @((Model).state.commands|Where-Object {$_.id -eq $Id -and $_.selected}).Count -eq 1
    } "Tool did not activate: $Id"
    if((Model).state.customization.drawer){
        $canvas=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-canvas'))
        $canvas.SetFocus();[CapyRowPointer]::Key($ProcessId,0x1B)
        Wait-Until {$null -eq (Model).state.customization.drawer -and !(Find 'tool-drawer' $null -Id)} 'Tool drawer did not dismiss after selection'
    }
}
function Check-Projection {
    # Both the snapshot and XAML can advance while asynchronous raster work settles.
    # Reacquire both: an old snapshot may describe a superseded transform schema.
    try{Wait-Until {
        try {
            $state=(Model).state
            if(!$state){return $false}
            foreach($set in @(@('groups','tool-group-'),@('subtools','tool-subtool-'))){
                $items=@($state.tool_set.($set[0]))
                for($i=0;$i -lt $items.Count;$i++){
                    $item=$items[$i];$native=Find ($set[1]+$i) -Id
                    if(!$native -or $native.Current.Name -ne $item.label -or (($native.Current.ItemStatus -eq 'Selected') -ne $item.selected)){return $false}
                }
                if(Find ($set[1]+$items.Count) -Id){return $false}
            }
            $groups=@($state.tool_set.groups);$subtools=@($state.tool_set.subtools)
            if($groups.Count -and $subtools.Count){
                $above=(Find ('tool-group-'+($groups.Count-1)) -Id).Current.BoundingRectangle;$below=(Find 'tool-subtool-0' -Id).Current.BoundingRectangle
                $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96
                if(($below.Top-$above.Bottom)/$scale -lt 12){return $false}
            }
            foreach($setting in $state.tool_settings){
                $native=Find ('tool-setting-'+$setting.id) ([System.Windows.Automation.ControlType]::Edit) -Id
                if(!$native -or !$native.Current.Name.Contains($setting.label)){return $false}
            }
            foreach($action in $state.tool_actions){
                $command=$state.commands|Where-Object id -eq $action.command
                $script:projectionIssue="action=$($action.command), group=$($action.group), segmented=$($action.segmented), checkable=$($action.checkable)"
                if($action.group -and !$action.segmented){
                    $native=Find ('tool-action-group-'+$action.group) ([System.Windows.Automation.ControlType]::ComboBox) -Id
                    if(!$native -or !$native.Current.Name){return $false}
                    if($command.selected){
                        $selected=$native.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection()
                        $script:projectionIssue+="; selected count=$($selected.Count), name=$($selected[0].Current.Name), expected=$($command.label), offscreen=$($native.Current.IsOffscreen)"
                        if($selected.Count -ne 1 -or $selected[0].Current.Name -ne $command.label){return $false}
                    }
                    continue
                }
                $mode=$action.segmented
                $type=if($mode){[System.Windows.Automation.ControlType]::Button}elseif($action.checkable){[System.Windows.Automation.ControlType]::CheckBox}else{[System.Windows.Automation.ControlType]::Button}
                $native=Find ('tool-action-'+$action.command) $type -Id
                if(!$native -or $native.Current.Name -ne $command.label -or $native.Current.IsEnabled -ne $command.enabled){return $false}
                if($mode){if(($native.Current.ItemStatus -eq 'Selected') -ne $command.selected){return $false};continue}
                if($action.checkable){
                    $checked=$native.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
                    if($checked -ne $command.selected){return $false}
                }
            }
            return $true
        }catch{$script:projectionIssue+="; $($_.Exception.Message)";return $false}
    } 'Native tool labels, selection, settings or actions did not reach the shared model'}catch{
        [IO.File]::WriteAllText((Join-Path $OutputDirectory 'tool-projection-failure.txt'),$script:projectionIssue)
        throw "$($_.Exception.Message): $script:projectionIssue"
    }
}
$toolGeometry=@()
function Check-Geometry([string]$Tool){
    $state=(Model).state
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $tonal=@($state.tool_extra|Where-Object {$_.Choice.id -eq 'tonal-tones'}).Count -gt 0
    $rows=@()
    foreach($setting in $state.tool_settings){
        $field=Find ('number-root-tool-setting-'+$setting.id) $null -Id
        $entry=Find ('tool-setting-'+$setting.id) ([System.Windows.Automation.ControlType]::Edit) -Id
        if(!$field -or !$entry){continue}
        $expected=if($setting.numeric.kind -ne 'slider' -or $tonal -or $setting.id -in @('transform_x','transform_y')){26}else{36}
        $field=Reveal-Number $setting.id $expected $scale
        $entry=Arranged ('tool-setting-'+$setting.id) ([System.Windows.Automation.ControlType]::Edit)
        $bounds=Native-Bounds ('number-root-tool-setting-'+$setting.id) $scale;$value=Native-Bounds ('tool-setting-'+$setting.id) $scale
        if([Math]::Abs($bounds.Height/$scale-$expected) -gt 1 -or [Math]::Abs($value.Height/$scale-26) -gt 1){throw "Tool $Tool field $($setting.id) does not use compact dimensions: root=$bounds entry=$value expected=$expected"}
        if($expected -eq 36 -and ([Math]::Abs($value.Width/$scale-68) -gt 1 -or [Math]::Abs($value.Bottom-$bounds.Bottom) -gt 1 -or [Math]::Abs($value.Right-$bounds.Right) -gt 1)){throw "Tool $Tool field $($setting.id) does not retain its editor width and bottom/trailing alignment"}
        $captions=$field.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))
        $caption=@($captions|Where-Object {$_.Current.Name -eq $setting.label})[0]
        if($caption -and [Math]::Abs((Native-Bounds ('number-caption-tool-setting-'+$setting.id) $scale).Left-$bounds.Left) -gt 1){throw "Tool $Tool field $($setting.id) indents its plain caption"}
        $rows+=@{id=$setting.id;height=$expected;bounds=$bounds;entry=$value;kind=$setting.numeric.kind}
    }
    $actionRows=@()
    foreach($group in @($state.tool_actions|Where-Object row|Group-Object row)){
        $buttons=@()
        foreach($action in $group.Group){
            $button=Find ('tool-action-'+$action.command) ([System.Windows.Automation.ControlType]::Button) -Id
            if($button){
                $button=Reveal-Control ('tool-action-'+$action.command) ('tool-action-'+$action.command) ([System.Windows.Automation.ControlType]::Button) 26 $scale
                $buttons+=@{id=$action.command;bounds=(Native-Bounds ('tool-action-'+$action.command) $scale)}
            }
        }
        if($buttons.Count -lt 2){continue}
        for($index=0;$index -lt $buttons.Count;$index++){
            $bounds=$buttons[$index].bounds
            if([Math]::Abs($bounds.Height/$scale-26) -gt 1){throw "Tool $Tool action $($buttons[$index].id) is not compact"}
            if($index -and ([Math]::Abs(($bounds.Left-$buttons[$index-1].bounds.Right)/$scale-4) -gt 1 -or [Math]::Abs($bounds.Top-$buttons[$index-1].bounds.Top) -gt 1)){throw "Tool $Tool row $($group.Name) does not use matching horizontal spacing"}
        }
        $actionRows+=@{id=$group.Name;icon_only=$group.Group[0].icon_only;buttons=$buttons}
    }
    foreach($row in $actionRows){foreach($button in $row.buttons){$button.bounds=Native-Bounds ('tool-action-'+$button.id) $scale}}
    $iconRows=@($actionRows|Where-Object icon_only|Sort-Object {$_.buttons[0].bounds.Top})
    for($index=1;$index -lt $iconRows.Count;$index++){
        if([Math]::Abs(($iconRows[$index].buttons[0].bounds.Top-$iconRows[$index-1].buttons[0].bounds.Bottom)/$scale-4) -gt 1){throw "Tool $Tool action rows do not match their horizontal spacing"}
    }
    $script:toolGeometry+=@{tool=$Tool;theme=$state.theme;dpi=96*$scale;rows=$rows;action_rows=$actionRows}
    $script:toolGeometry|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'tool-settings-geometry.json')
    $group=@((Model).layout.groups|Where-Object active -eq 'tool_settings')[0]
    $origin=[CapyRowPointer+Point]::new();[CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)|Out-Null
    @{client_origin=$origin;panel=(Native-Bounds $group.id $scale -Group);dpi=96*$scale}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $OutputDirectory ('tool-'+$Tool+'-capture-layout.json'))
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $ProcessId -WindowHandle $drawingWindow.ToInt64() -Output (Join-Path $OutputDirectory ('tool-'+$Tool+'.png')) -ClientOnly -CaptureOnly *> (Join-Path $OutputDirectory ('tool-'+$Tool+'-capture.json'))
    if($rows.Count -gt 5){
        $first=$rows[0]
        $null=Reveal-Number $first.id $first.height $scale
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $ProcessId -WindowHandle $drawingWindow.ToInt64() -Output (Join-Path $OutputDirectory ('tool-'+$Tool+'-top.png')) -ClientOnly -CaptureOnly *> (Join-Path $OutputDirectory ('tool-'+$Tool+'-top-capture.json'))
    }
}
if(!@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count){
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $window=(Model).application_menus|Where-Object id -eq 'window'
    $toolMenu=$window.model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.panel -eq 'tool_settings'}
    if(!$toolMenu){throw 'Shared Tool panel menu is missing'}
    (Control $toolMenu.label ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains 'tool_settings'}).Count -gt 0} 'Tool panel did not open'
}
if(!@((Model).layout.groups|Where-Object active -eq 'tool_settings').Count){Invoke-Id 'panel-tab-tool_settings'}
Wait-Until {@((Model).layout.groups|Where-Object active -eq 'tool_settings').Count -eq 1} 'Tool panel did not activate'
Select-Tool 'pen'
$null=Field 'flow'
$entry=Field 'flow';$original=$entry.GetRuntimeId() -join ':'
$subtool=(Control 'tool-subtool-0' -Id).GetRuntimeId() -join ':'
$before=Model
$ownership=[ordered]@{before=@{field=$original;subtool=$subtool;state=$before.state;workspace=$before.windows_workspace}}
Draft 'flow' '40 + 2'
Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-.42) -lt .0001} 'Flow expression was not committed through the shared core'
$after=Model
$ownership.after=@{field=((Field 'flow').GetRuntimeId() -join ':');subtool=((Control 'tool-subtool-0' -Id).GetRuntimeId() -join ':');state=$after.state;workspace=$after.windows_workspace}
$ownership|ConvertTo-Json -Depth 50|Set-Content (Join-Path $OutputDirectory 'tool-flow-ownership.json')
if(((Field 'flow').GetRuntimeId() -join ':') -ne $original){throw 'A value update replaced the tool field'}
if(((Control 'tool-subtool-0' -Id).GetRuntimeId() -join ':') -ne $subtool){throw 'A value update replaced the subtool button'}
# A range edit replaces an invalid text draft and must keep the formatted units.
Draft 'flow' 'invalid'
$flowSlider=Control 'tool-setting-flow-slider' ([System.Windows.Automation.ControlType]::Slider) -Id
Focus $flowSlider
$flowSlider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(1)
Wait-Until {[Math]::Abs((Value 'flow')-1) -lt .0001} 'Slider did not replace the numeric draft'
Focus (Field 'opacity')
Wait-Until {(Field 'flow').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '100 %'} 'Slider lost units or retained the invalid numeric draft'
Draft 'flow' '42';Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-.42) -lt .0001} 'Flow did not restore after the slider draft check'
# Native focus can leave the field before an accessibility invocation runs.
# That may commit to the old brush; the detached draft must never overwrite
# the newly selected brush, including when focus moves again afterwards.
Select-Preset 7
$markerFlow=Value 'flow'
Select-Preset 1
Draft 'flow' '77'
Select-Preset 7
Check-Projection
Focus (Field 'opacity')
Wait-Until {[Math]::Abs((Value 'flow')-$markerFlow) -lt .0001} 'Detached draft overwrote the new brush'
Select-Preset 1
if(((Field 'flow').GetRuntimeId() -join ':') -eq $original){throw 'Changing tools retained an old draft context'}
if(!@((Model).layout.groups|Where-Object active -eq 'layers').Count){
    Invoke-Id 'column-icon-layers'
    Wait-Until {@((Model).layout.groups|Where-Object active -eq 'layers').Count -gt 0} 'Layers column did not open'
}
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
$tools=@('pen','pencil','brush','eraser','airbrush','blend','liquify','lasso','move','hand','eyedropper','gradient','figure','ruler','auto_select','fill')
foreach($tool in $tools){
    Select-Tool $tool;Check-Projection
    Check-Geometry $tool
    if($tool -eq 'liquify'){
        foreach($preset in @(63,64)){
            $choice=@((Model).state.tool_set.subtools|Where-Object preview -eq $preset)
            if($choice.Count -ne 1){throw "Liquify preset $preset is missing from the native Tool Set"}
            $index=0
            while((Model).state.tool_set.subtools[$index].preview -ne $preset){$index++}
            Invoke-Id ('tool-subtool-'+$index)
            Wait-Until {(Model).state.brush.preset -eq $preset -and (Model).brush_ready} "Liquify preset $preset did not activate"
            Check-Projection
            $ids=@((Model).state.tool_settings|ForEach-Object id)
            if('size' -notin $ids -or 'strength' -notin $ids){throw "Liquify preset $preset lacks Size or Strength"}
            if($preset -eq 64 -and @($ids|Where-Object {$_ -in @('distortion','momentum')}).Count){throw 'Reconstruct exposes deformation-only controls'}
            if($preset -eq 63 -and @($ids|Where-Object {$_ -in @('distortion','momentum')}).Count -ne 2){throw 'Edge lacks Distortion or Momentum'}
            foreach($command in @('liquify_adjust','liquify_reset')){
                if(!@((Model).state.tool_actions|Where-Object command -eq $command).Count){throw "Liquify action $command is missing"}
            }
            Check-Geometry ("liquify-$preset")
        }
    }
    if($tool -eq 'eyedropper'){
        if((Model).state.layer_tools.tool -notlike 'pick_*'){throw 'Eyedropper did not start temporary picking'}
        Select-Tool 'eyedropper' -Cancel
        Wait-Until {(Model).state.layer_tools.tool -notlike 'pick_*'} 'Eyedropper did not cancel temporary picking'
    }
}
$toolGeometry|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'tool-settings-geometry.json')
# Exercise real keyboard routing through the native spin field and Rust policy.
Select-Tool 'auto_select'
Draft 'gap_closing' '6 * 2'
[CapyRowPointer]::Key($ProcessId,0x0D)
Wait-Until {(Value 'gap_closing') -eq 12} 'Enter did not commit the spin expression'
[CapyRowPointer]::Key($ProcessId,0x26)
Wait-Until {(Value 'gap_closing') -eq 13} 'Up did not step the spin control'
[CapyRowPointer]::Key($ProcessId,0x28)
Wait-Until {(Value 'gap_closing') -eq 12} 'Down did not step the spin control'
Draft 'gap_closing' 'invalid'
[CapyRowPointer]::Key($ProcessId,0x26)
Wait-Until {(Field 'gap_closing').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'invalid'} 'Invalid spin input was discarded'
[CapyRowPointer]::Key($ProcessId,0x1B)
Wait-Until {(Field 'gap_closing').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq '12'} 'Escape did not cancel the invalid spin draft'
if((Value 'gap_closing') -ne 12){throw 'Invalid spin input changed the shared value'}
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
# The automation scroll request focuses the first numeric field. Let that
# field's queued bring-into-view finish before issuing the explicit scroll.
Focus (Field 'size')
$settled=@{key='';count=0}
Wait-Until {
    $geometry=@($scroll.Current.VerticalScrollPercent,$scroll.Current.VerticalViewSize,(Field 'size').Current.BoundingRectangle.ToString()) -join ':'
    if($geometry -eq $settled.key){$settled.count++}else{$settled.key=$geometry;$settled.count=0}
    $settled.count -ge 3
} 'Initial numeric focus did not settle'
$scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,100)
Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-100) -lt .01} 'Tool panel did not finish scrolling'
$scrollIdentity=$cursor.GetRuntimeId() -join ':'
$slider=$null;$sliderId=$null
$settings=@((Model).state.tool_settings);[array]::Reverse($settings)
for($step=0;!$slider -and $step -lt 30;$step++){
    foreach($setting in $settings){
        $candidate=Find ('tool-setting-'+$setting.id+'-slider') ([System.Windows.Automation.ControlType]::Slider) -Id
        if($candidate -and !$candidate.Current.IsOffscreen){$slider=$candidate;$sliderId=$setting.id;break}
    }
    if(!$slider){
        $previous=$scroll.Current.VerticalScrollPercent
        $scroll.Scroll([System.Windows.Automation.ScrollAmount]::NoAmount,[System.Windows.Automation.ScrollAmount]::SmallDecrement)
        Wait-Until {$scroll.Current.VerticalScrollPercent -lt $previous} 'Tool panel did not scroll up toward a slider'
    }
}
if(!$slider){throw 'Scroll retention requires a visible slider'}
$percent=$scroll.Current.VerticalScrollPercent
if($percent -le 0){throw 'Scroll retention requires a scrolled panel'}
$sliderBefore=Value $sliderId
Focus $slider
Wait-Until {[Math]::Abs($scroll.Current.VerticalScrollPercent-$percent) -lt .01} 'Focusing the visible slider changed scrolling'
$range=$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern)
$range.SetValue($(if($range.Current.Value -gt .5){.2}else{.8}))
Wait-Until {(Value $sliderId) -ne $sliderBefore} "Tool slider did not update shared $sliderId"
if(($cursor.GetRuntimeId() -join ':') -ne $scrollIdentity -or [Math]::Abs($scroll.Current.VerticalScrollPercent-$percent) -gt .01){throw 'Tool edit replaced or reset scrolling'}
Select-Tool 'gradient'
if(@((Model).state.tool_set.groups).Count){throw 'Gradient shapes kept category buttons'}
Invoke-Id 'tool-subtool-1'
Wait-Until {(Model).state.tool_set.subtools[1].selected} 'Radial gradient did not select'
Check-Projection
function Tool-Gradient{(@((Model).state.tool_extra|Where-Object {$_.Gradient})[0]).Gradient.value.value|ConvertTo-Json -Depth 8 -Compress}
if(!(Tool-Gradient)){throw 'Gradient tool settings lack the shared gradient'}
Invoke-Id 'tool-gradient-reset'
$settled=@{value=$null};Wait-Until {$value=Tool-Gradient;$same=$value -eq $settled.value;$settled.value=$value;Start-Sleep -Milliseconds 200;$same} 'Tool gradient Reset did not settle'
$gradient=$settled.value
Invoke-Id 'tool-gradient-reverse'
Wait-Until {(Tool-Gradient) -ne $gradient} 'Tool gradient Reverse did not apply'
Invoke-Id 'tool-gradient-reset'
Wait-Until {(Tool-Gradient) -eq $gradient} 'Tool gradient Reset did not restore the paint pair'
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
Wait-Until {(Model).brush_ready} 'Selected pen did not become ready'
Invoke-Control 'Test stroke'
Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq 'undo' -and $_.enabled}).Count -eq 1} 'Controlled stroke did not complete'
Select-Tool 'scale_rotate'
Check-Projection
Check-Geometry 'scale_rotate'
$toolGeometry|ConvertTo-Json -Depth 8|Set-Content (Join-Path $OutputDirectory 'tool-settings-geometry.json')
Invoke-Id 'tool-action-cancel_transform'
Wait-Until {@((Model).state.tool_actions|Where-Object command -eq 'cancel_transform').Count -eq 0} 'Transform cancel did not finish'
Select-Tool 'pen'
[pscustomobject]@{tool_and_subtool_projection='passed';shared_numeric_expression='passed';spin_keyboard_editing='passed';slider_cancels_numeric_draft='passed';retained_fields_and_buttons='passed';retained_scrolling='passed';
    stale_tool_draft='passed';target_context_replacement='passed';figure_schema_change='passed';gradient_subtools='passed';ruler_toggle='passed';transform_cancel='passed';
    scope='native UI Automation and shared state; physical input and full-editor visual parity remain open'}|ConvertTo-Json
