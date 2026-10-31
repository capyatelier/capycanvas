param([int]$ProcessId,[long]$WindowHandle,[string]$StateFile,[string]$EvidenceDirectory,[string]$Executable)
$ErrorActionPreference='Stop'
if($Executable){& (Join-Path $PSScriptRoot 'exercise-header.ps1') -Executable $Executable -Journey catalog-only;return}
if(!$ProcessId -or !$WindowHandle -or !$StateFile){throw 'Supply an executable or an owned process, window and state file'}
if(!$EvidenceDirectory){$EvidenceDirectory=Split-Path -Parent $StateFile}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected a controlled CapyCanvas review process.'}
$app.Refresh()
if($app.HasExited -or !$WindowHandle){throw 'The controlled drawing window is unavailable'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowHandle)
if(!$root -or $root.Current.ProcessId -ne $ProcessId){throw 'The drawing window does not belong to this review'}
$script:settingsScope=$root
$script:swatchAllocations=[Collections.Generic.List[object]]::new()
function Read-Model {
    try {
        $snapshot=Read-Snapshot $StateFile
        if($snapshot.process_id -ne $ProcessId){return $null}
        return $snapshot.model
    } catch {return $null}
}
function Find-Control([string]$Name,$Type) {
    $nameCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name)
    $typeCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type)
    $script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new($nameCondition,$typeCondition))
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$TimeoutSeconds=5) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {if(& $Condition){return};Start-Sleep -Milliseconds 75} while($watch.Elapsed.TotalSeconds -lt $TimeoutSeconds)
    throw $Message
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $hit=@{element=$null}
    Wait-Until {$hit.element=Find-Control $Name $Type;$null -ne $hit.element} "Missing control: $Name"
    $hit.element
}
function Invoke-Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $item=Control $Name $Type;$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Toggle-Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    (Control $Name $Type).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
}
function Toggle-State([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    (Control $Name $Type).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState
}
function Read-Text([string]$Name) {
    (Control $Name ([System.Windows.Automation.ControlType]::Edit)).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
}
function Focus-Control($Entry) {
    $Entry.SetFocus()
    Wait-Until {$Entry.Current.HasKeyboardFocus} 'Native control did not receive focus'
}
function Edit-Text([string]$Name,[string]$Value) {
    $entry=Control $Name ([System.Windows.Automation.ControlType]::Edit)
    Focus-Control $entry
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
}
function Custom-Swatch([string]$Theme) {
    Wait-Until {$script:swatch=@($script:settingsScope.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like "setting-$($Theme)_base-swatch-*"})[0];$script:swatch} "Missing custom $Theme base color"
    $script:swatch
}
function Preference-Row([string]$Id) {
    @((Read-Model).preferences.pages|ForEach-Object groups|ForEach-Object rows|Where-Object id -eq $Id)[0]
}
function Swatch-Button([string]$Id,[int]$Index) {
    $found=@{element=$null}
    Wait-Until {$found.element=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,"setting-$Id-swatch-$Index"));$found.element} "Missing native swatch $Id/$Index"
    $found.element
}
function Swatch-Bounds($Bounds,$Origin,[double]$Scale) {
    @{x=$Origin.x+$Bounds.x*$Scale;top=$Origin.y+$Bounds.y*$Scale;bottom=$Origin.y+($Bounds.y+$Bounds.height)*$Scale;width=$Bounds.width*$Scale;height=$Bounds.height*$Scale}
}
function Swatch-Choice([string]$Id,[int]$Index) {
    $button=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,"setting-$Id-swatch-$Index"))
    if($button){$current=$button.Current;@{id=$current.AutomationId;name=$current.Name;status=$current.ItemStatus;process=$current.ProcessId;offscreen=$current.IsOffscreen;enabled=$current.IsEnabled;runtime_id=($button.GetRuntimeId()-join ':')}}
}
function Preset-Acknowledged($Row,[int]$Index,[string]$Value,$Choice) {
    $Row -and $Choice -and $Row.kind.value -eq $Value -and $Row.kind.selected -eq $Index -and $Choice.process -eq $ProcessId -and $Choice.id -ceq "setting-$($Row.id)-swatch-$Index" -and $Choice.name -ceq $Row.kind.swatches[$Index].label -and $Choice.status -ceq 'Selected'
}
function Swatch-Allocation([string]$Id,[double]$Scale) {
    $title=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'settings-title'))
    if(!$title -or $title.Current.IsOffscreen -or !$title.Current.ItemStatus){return}
    if($title.Current.ProcessId -ne $ProcessId){throw 'Native Preferences allocation belongs to another process'}
    $trace=$title.Current.ItemStatus|ConvertFrom-Json
    $parts=@($trace.rows|Where-Object id -eq $Id)
    if($parts.Count -ne 1 -or $trace.title.width -le 0 -or $trace.title.height -le 0){return}
    $anchor=$title.Current.BoundingRectangle
    if($anchor.IsEmpty){return}
    $origin=@{x=$anchor.X-$trace.title.x*$Scale;y=$anchor.Y-$trace.title.y*$Scale}
    @{row=(Swatch-Bounds $parts[0].row $origin $Scale);strip=(Swatch-Bounds $parts[0].strip $origin $Scale);entry=$(if($parts[0].entry_visible){Swatch-Bounds $parts[0].entry $origin $Scale});entry_in_strip=[bool]$parts[0].entry_in_strip;native=$parts[0];anchor_runtime_id=($title.GetRuntimeId()-join ':')}
}
function Wait-SwatchAllocation([string]$Id,$Row,[double]$Scale,[bool]$Open,[int]$PresetIndex=-1,[string]$PresetValue) {
    $stable=@{key='';value=$null;watch=[Diagnostics.Stopwatch]::StartNew();attempts=0;key_changes=0;last=$null}
    try{
        Wait-Until {
            $stable.attempts++
            $currentRow=Preference-Row $Id
            $choiceIndex=if($Open){$currentRow.kind.swatches.Count-1}elseif($PresetIndex -ge 0){$PresetIndex}else{[int]$currentRow.kind.selected}
            $sample=@{gate='preset_acknowledgement';shared_row=$currentRow;native_button=(Swatch-Choice $Id $choiceIndex);expected_preset=$PresetIndex;expected_value=$PresetValue;allocation_found=$false;allocation=$null;native_entry_visible=$false;native_entry_present=$false;uia=$null;stable_elapsed_ms=0}
            $stable.last=$sample
            if($PresetIndex -ge 0 -and !(Preset-Acknowledged $currentRow $PresetIndex $PresetValue $sample.native_button)){return $false}
            $sample.gate='native_allocation';$value=Swatch-Allocation $Id $Scale;$sample.allocation_found=$null -ne $value;$sample.allocation=$value
            if(!$value){return $false}
            $sample.native_entry_visible=[bool]$value.native.entry_visible;$sample.native_entry_present=[bool]$value.entry;$sample.gate='uia_entry'
            $entry=Find-Control $Row.title ([System.Windows.Automation.ControlType]::Edit)
            $visible=$entry -and !$entry.Current.IsOffscreen -and $entry.Current.BoundingRectangle.Height -gt 0
            $sample.uia=if($entry){$current=$entry.Current;@{found=$true;id=$current.AutomationId;process=$current.ProcessId;offscreen=$current.IsOffscreen;bounds=$current.BoundingRectangle;visible=[bool]$visible;runtime_id=($entry.GetRuntimeId()-join ':')}}else{@{found=$false;visible=$false}}
            $sample.gate='visible_state'
            if([bool]$visible -ne $Open -or [bool]$value.entry -ne $Open){return $false}
            $sample.gate='entry_identity_and_height'
            if($Open -and ($entry.Current.ProcessId -ne $ProcessId -or $entry.Current.AutomationId -ne "setting-text-$Id" -or $value.entry.height -le 0)){return $false}
            $sample.gate='row_and_strip_height'
            if($value.row.height -le 0 -or $value.strip.height -le 0){return $false}
            $key=$value|ConvertTo-Json -Depth 5 -Compress
            if($key -ne $stable.key){$stable.key=$key;$stable.watch.Restart();$stable.key_changes++};$stable.value=$value
            $sample.gate='stability';$sample.stable_elapsed_ms=$stable.watch.ElapsedMilliseconds
            $stable.watch.ElapsedMilliseconds -ge 250
        } "Native $Id allocation did not settle with its editor open=$Open"
    }catch{
        $receipt=@{id=$Id;title=$Row.title;expected_process=$ProcessId;requested_open=[bool]$Open;scale=$Scale;attempts=$stable.attempts;key_changes=$stable.key_changes;last=$stable.last;failure=($_|Out-String)}
        [IO.File]::WriteAllText((Join-Path $EvidenceDirectory ("swatch-allocation-$Id-$Open-failure.json")),($receipt|ConvertTo-Json -Depth 15),[Text.UTF8Encoding]::new($false))
        throw
    }
    $stable.value
}
function Assert-SwatchAllocation($Closed,$Open,$ClosedAgain,[double]$Gap,[double]$Scale,[bool]$Inline) {
    if([double]::IsNaN($Scale) -or [double]::IsInfinity($Scale) -or $Scale -le 0 -or !$Open.entry -or $Open.entry_in_strip){throw 'Native swatch allocation does not isolate its revealed entry'}
    $inset=$Open.row.bottom-$Open.entry.bottom
    $metrics=if($Inline){[ordered]@{gap=($Open.strip.x-$Open.entry.x-$Open.entry.width)/$Scale;entry_alignment=(($Open.entry.top+$Open.entry.bottom)-($Open.strip.top+$Open.strip.bottom))/(2*$Scale);row_height_change=($Open.row.height-$Closed.row.height)/$Scale;closed_height_change=($ClosedAgain.row.height-$Closed.row.height)/$Scale;strip_height_change=($Open.strip.height-$Closed.strip.height)/$Scale;row_width_change=($Open.row.width-$Closed.row.width)/$Scale}}else{[ordered]@{gap=($Open.entry.top-$Open.strip.bottom)/$Scale;growth=($Open.row.height-$Closed.row.height-$Open.entry.height)/$Scale;closed_excess=($Closed.row.bottom-$Closed.strip.bottom-$inset)/$Scale;closed_again_excess=($ClosedAgain.row.bottom-$ClosedAgain.strip.bottom-$inset)/$Scale;closed_height_change=($ClosedAgain.row.height-$Closed.row.height)/$Scale;strip_height_change=($Open.strip.height-$Closed.strip.height)/$Scale;row_width_change=($Open.row.width-$Closed.row.width)/$Scale}}
    foreach($name in $metrics.Keys){$expected=if($name -in @('gap','growth')){$Gap}else{0};if([double]::IsNaN($metrics[$name]) -or [double]::IsInfinity($metrics[$name]) -or [Math]::Abs($metrics[$name]-$expected) -gt 1./$Scale){throw "Native custom-color allocation differs at ${name}: $($metrics|ConvertTo-Json -Compress)"}}
    $metrics
}
function Check-SwatchAllocation([string]$Id) {
    $row=Preference-Row $Id;if(!$row -or $row.kind.type -ne 'swatches'){throw "Shared swatch row is missing for $Id"}
    $saved=$row.kind.value;$selected=[int]$row.kind.selected;$custom=$row.kind.swatches.Count-1
    if(!$row.kind.swatches[$custom].custom){throw 'Shared custom swatch is not the final native choice'}
    $record=[ordered]@{theme=(Read-Model).state.theme;id=$Id;scale=([CapyRowPointer]::GetDpiForWindow(([IntPtr]$WindowHandle))/96.);closed=$null;open=$null;closed_again=$null;metrics=$null}
    $script:swatchAllocations.Add($record)
    try{
        $preset=Swatch-Button $Id 0;$scroll=$null;if($preset.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
        $preset.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $record.source='settings-title native allocations'
        $record.closed=Wait-SwatchAllocation $Id $row $record.scale $false 0 $row.kind.swatches[0].value
        (Swatch-Button $Id $custom).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $record.open=Wait-SwatchAllocation $Id $row $record.scale $true
        if(!(Control $row.title ([System.Windows.Automation.ControlType]::Edit)).Current.HasKeyboardFocus){throw 'Opening Custom did not focus the retained native field'}
        (Swatch-Button $Id 0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $record.closed_again=Wait-SwatchAllocation $Id $row $record.scale $false 0 $row.kind.swatches[0].value
        $gap=if($row.kind.inline){8.}else{12.};$record.metrics=Assert-SwatchAllocation $record.closed $record.open $record.closed_again $gap $record.scale ([bool]$row.kind.inline)
    }finally{
        try{
            (Swatch-Button $Id $selected).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            if($row.kind.swatches[$selected].custom){Edit-Text $row.title $saved;Focus-Control (Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox))}
            $restoration=@{shared_row=$null;native_button=$null;expected_index=$selected;expected_value=$saved}
            try{Wait-Until {$restoration.shared_row=Preference-Row $Id;$restoration.native_button=Swatch-Choice $Id $selected;Preset-Acknowledged $restoration.shared_row $selected $saved $restoration.native_button} "Native $Id swatch did not restore its original shared value"}
            catch{[IO.File]::WriteAllText((Join-Path $EvidenceDirectory ("swatch-restoration-$Id-failure.json")),($restoration|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false));throw}
        }finally{[IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'header-settings-custom-colors.json'),(@($script:swatchAllocations.ToArray())|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))}
    }
}
function Open-Preferences {
    $script:settingsScope=$root
    $button=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'settings-button'))
    if($button -and !$button.Current.IsOffscreen){
        $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }else{
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
        Invoke-Control 'Preferences' ([System.Windows.Automation.ControlType]::MenuItem)
    }
    $script:settingsScope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
}
function Close-Preferences {
    # The shortcut list also has a command row named Close. Address the native
    # ContentDialog footer, not a same-named shortcut or nested editor action.
    $close=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'))
    if(!$close){throw 'Preferences footer Close button is missing'}
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    $script:settingsScope=$root
    Wait-Until {!(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
Wait-Until {Read-Model} 'Launch this review instance with CAPY_TRACE_UI=1 and pass its ui-state.json file' 30
Wait-Until {(Read-Model).brush_ready} 'Shared brush startup did not finish before interaction checks' 45
if(!(Read-Model).windows_isolated_settings){throw 'Launch this fixture with CAPY_STORAGE_DIR pointing to a disposable profile.'}
if(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)){throw 'Close Preferences before running this fixture.'}
try{& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'}catch{
    $current=$root.Current
    $nodes=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -match '^(application-menu|header-recovery-menu)'}|ForEach-Object {$item=$_.Current;@{id=$item.AutomationId;name=$item.Name;process=$item.ProcessId;offscreen=$item.IsOffscreen;enabled=$item.IsEnabled}})
    [IO.File]::WriteAllText((Join-Path $EvidenceDirectory 'settings-menu-root-failure.json'),(@{provided_hwnd=$WindowHandle;root_hwnd=$current.NativeWindowHandle;root_name=$current.Name;root_process=$current.ProcessId;menu_nodes=$nodes}|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
    throw
}
if(Find-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)){throw 'View must not include Dark Mode'}
Open-Preferences
$originalTheme=(Read-Model).state.settings.theme
$restoreTheme=if($originalTheme -eq 'dark'){'Dark'}elseif($originalTheme -eq 'light'){'Light'}else{'System'}
$themeIdentity=(Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).GetRuntimeId() -join ':'
foreach($choice in @('Light','Dark',$restoreTheme)){
    [CapyRowPointer]::SetForegroundWindow(([IntPtr]$WindowHandle))|Out-Null
    Focus-Control (Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox))
    [CapyRowPointer]::Key([uint32]$ProcessId,[uint16]0x73) # F4 opens the native selector.
    Wait-Until {Find-Control $choice ([System.Windows.Automation.ControlType]::ListItem)} 'Theme choices did not open'
    [CapyRowPointer]::Key([uint32]$ProcessId,[uint16]0x24) # Home: System.
    $index=@('System','Light','Dark').IndexOf($choice)
    for($i=0;$i -lt $index;$i++){[CapyRowPointer]::Key([uint32]$ProcessId,[uint16]0x28)}
    [CapyRowPointer]::Key([uint32]$ProcessId,[uint16]0x0D)
    Wait-Until {(Read-Model).state.settings.theme -eq $(if($choice -eq 'System'){$null}else{$choice.ToLowerInvariant()})} 'Color theme preference did not update'
    $script:settingsScope=$root
    $script:settingsScope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
    Wait-Until {(Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).Current.HasKeyboardFocus} 'Theme change moved focus out of its preference'
    if(((Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).GetRuntimeId() -join ':') -ne $themeIdentity){throw 'Theme selection replaced its native control'}
    $selected=(Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection()
    if($selected.Count -ne 1 -or $selected[0].Current.Name -ne $choice){throw 'Collapsed theme selector has no readable selected value'}
    Check-SwatchAllocation ((Read-Model).state.theme+'_base')
    Check-SwatchAllocation 'accent'
}
(Custom-Swatch 'dark').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
Wait-Until {Find-Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)} 'Custom dark base color did not open its field'
$base=Read-Text 'Dark theme base color'
$entry=Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)
$identity=$entry.GetRuntimeId() -join ':'
Edit-Text 'Dark theme base color' '#1d2d3d'
Focus-Control (Custom-Swatch 'light')
Wait-Until {(Read-Model).state.settings.dark_base -eq '#1d2d3d'} 'Valid base color did not reach shared settings'
Wait-Until {(Read-Text 'Dark theme base color') -eq '#1d2d3d'} 'Valid base color was not retained'
if(((Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)).GetRuntimeId() -join ':') -ne $identity){throw 'Palette update replaced the native settings field'}
Edit-Text 'Dark theme base color' 'invalid'
Focus-Control (Custom-Swatch 'light')
Wait-Until {(Read-Model).preferences.error} 'Invalid color did not produce shared validation feedback'
Wait-Until {(Read-Text 'Dark theme base color') -eq '#1d2d3d'} 'Invalid base color replaced the valid value'
Edit-Text 'Dark theme base color' $base
Focus-Control (Custom-Swatch 'light')
Wait-Until {(Read-Model).state.settings.dark_base -eq $base} 'Base color did not restore in shared settings'

$tiles=@('Looking up','Facing forward','Bathing','Sleeping')
$selected=$tiles | Where-Object {(Toggle-State $_) -eq [System.Windows.Automation.ToggleState]::On}
if(@($selected).Count -ne 1){throw 'Icon picker must have one selected tile'}
$choice=($tiles | Where-Object {$_ -ne $selected})[0]
$originalIcon=(Read-Model).state.settings.zen_icon
Toggle-Control $choice
Wait-Until {(Read-Model).state.settings.zen_icon -ne $originalIcon} 'Tile did not update the shared icon'
# A round trip through shared page state distinguishes a native toggle from a model edit.
Invoke-Control 'Canvas'
Invoke-Control 'Appearance'
Wait-Until {(Toggle-State $choice) -eq [System.Windows.Automation.ToggleState]::On} 'Icon selection did not reach shared preferences'
if((Toggle-State $selected) -ne [System.Windows.Automation.ToggleState]::Off){throw 'Icon selection was not exclusive'}
Toggle-Control $selected
Wait-Until {(Read-Model).state.settings.zen_icon -eq $originalIcon} 'Icon restoration failed'

Invoke-Control 'Search preferences'
Edit-Text 'Search preferences' 'prediction'
$prediction=Preference-Row 'prediction_horizon'
if(!$prediction -or $prediction.kind.type -ne 'number' -or $prediction.kind.control.kind -ne 'slider'){throw 'The shared prediction amount row is not a slider'}
Wait-Until {Find-Control $prediction.title ([System.Windows.Automation.ControlType]::Button)} 'Shared search results missing'
Invoke-Control $prediction.title
Wait-Until {Find-Control (Preference-Row 'prediction_horizon').title ([System.Windows.Automation.ControlType]::Slider)} 'Search result did not reveal the prediction slider'
$feedback=(Read-Model).state.settings.feedback
Toggle-Control 'Enable stroke prediction'
Wait-Until {(Read-Model).state.settings.feedback -ne $feedback} 'Preview toggle did not reach shared settings'
Wait-Until {$prediction=Preference-Row 'prediction_horizon';(Control $prediction.title ([System.Windows.Automation.ControlType]::Slider)).Current.IsEnabled -eq $prediction.enabled} 'Prediction slider has the wrong enabled state'
Toggle-Control 'Enable stroke prediction'
Wait-Until {(Read-Model).state.settings.feedback -eq $feedback} 'Preview restoration failed'
Close-Preferences

# Reopen as soon as the popup disappears, while the previous close animation
# may still own the window's ContentDialog slot.
1..3 | ForEach-Object {Open-Preferences;Close-Preferences}
Open-Preferences
Invoke-Control 'Keyboard Shortcuts'
Edit-Text 'Search shortcuts' 'Undo'
Invoke-Control 'Undo'
Wait-Until {(Read-Model).preferences.shortcut_editor} 'Shortcut editor did not open'
Invoke-Control 'Add Shortcut'
Wait-Until {(Read-Model).preferences.capture} 'Shortcut capture did not open'
foreach($chord in @(@(0x20,'Space'),@(0x0D,'Enter'))){
    [CapyRowPointer]::Key([uint32]$ProcessId,[uint16]$chord[0])
    Wait-Until {$capture=(Read-Model).preferences.capture;$capture -and $capture.shortcut -eq $chord[1]} "Shortcut capture did not record $($chord[1])"
}
Invoke-Control 'Cancel'
Wait-Until {!(Read-Model).preferences.capture -and (Read-Model).preferences.shortcut_editor} 'Cancel did not return to the shortcut editor'
$editorClose=@{element=$null}
Wait-Until {$editorClose.element=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'shortcut-editor-close'));$editorClose.element -and $editorClose.element.Current.IsEnabled -and !$editorClose.element.Current.IsOffscreen} 'Nested shortcut Close button is missing'
$editorClose.element.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
Wait-Until {$preferences=(Read-Model).preferences;$preferences -and !$preferences.shortcut_editor} 'Close did not dismiss only the nested shortcut editor'
Close-Preferences

# Fullscreen and status-item visibility are exercised by exercise-header.ps1.
# Native Preferences must not retain the obsolete global visibility control.
Open-Preferences
Invoke-Control 'Appearance'
if(Find-Control 'Show battery and clock' ([System.Windows.Automation.ControlType]::ComboBox)){throw 'Workspace status visibility leaked into global Preferences'}
Close-Preferences
[pscustomobject]@{
    view_menu_without_theme='passed'
    preferences_theme_roundtrip='passed'
    preferences_theme_focus='passed'
    preferences_theme_accessible_value='passed'
    settings_color_validation='passed'
    retained_settings_field='passed'
    custom_color_allocations=@($script:swatchAllocations.ToArray())
    exclusive_icon_tiles='passed'
    shared_settings_search='passed'
    dependent_settings_controls='passed'
    dialog_reopen_roundtrip='passed'
    shortcut_editor_cancel='passed'
    workspace_status_policy='passed'
    input_method='guarded OS keyboard for theme selection; native UI Automation for other controls'
} | ConvertTo-Json
