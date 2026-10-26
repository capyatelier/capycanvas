param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected a controlled CapyCanvas review process.'}
$watch=[Diagnostics.Stopwatch]::StartNew()
do {$app.Refresh();if($app.HasExited){throw 'Review app exited'};if($app.MainWindowHandle -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 100} while($watch.Elapsed.TotalSeconds -lt 30)
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
$script:settingsScope=$root
$script:swatchAllocations=[Collections.Generic.List[object]]::new()
function Read-Model {
    try {
        $snapshot=Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json
        if($snapshot.process_id -ne $ProcessId){return $null}
        return $snapshot.model
    } catch {return $null} # The opt-in snapshot may be finishing a write.
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
    (Control $Name $Type).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
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
function Swatch-Row([string]$Id) {
    @((Read-Model).preferences.pages|ForEach-Object groups|ForEach-Object rows|Where-Object id -eq $Id)[0]
}
function Swatch-Button([string]$Id,[int]$Index) {
    $found=@{element=$null}
    Wait-Until {$found.element=$script:settingsScope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,"setting-$Id-swatch-$Index"));$found.element} "Missing native swatch $Id/$Index"
    $found.element
}
function Swatch-Frames([string]$Id,$Row) {
    $walker=[System.Windows.Automation.TreeWalker]::RawViewWalker;$strip=$null;$scope=$script:settingsScope.GetRuntimeId()-join ':'
    for($node=$walker.GetParent((Swatch-Button $Id 0));$node -and ($node.GetRuntimeId()-join ':') -ne $scope;$node=$walker.GetParent($node)){
        $children=@($node.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition))
        $swatches=@($children|Where-Object {$_.Current.AutomationId -like 'setting-*-swatch-*'})
        if($swatches.Count -ne $Row.kind.swatches.Count -or @($swatches|Where-Object {$_.Current.AutomationId -notlike "setting-$Id-swatch-*"}).Count){continue}
        $heading=@($children|Where-Object {$_.Current.ControlType -eq [System.Windows.Automation.ControlType]::Text -and $_.Current.Name -eq $Row.title})
        if($heading.Count){if(!$strip){throw "Native swatch strip allocation is unavailable for $Id"};return @{row=$node;strip=$strip}}
        if(!$strip){$strip=$node}
    }
    throw "Native enclosing swatch allocation is unavailable for $Id"
}
function Swatch-Bounds($Element) {
    $b=$Element.Current.BoundingRectangle
    @{x=$b.X;top=$b.Top;bottom=$b.Bottom;width=$b.Width;height=$b.Height}
}
function Wait-SwatchAllocation([string]$Id,$Row,$Frames,[bool]$Open) {
    $stable=@{key='';value=$null;watch=[Diagnostics.Stopwatch]::StartNew()}
    Wait-Until {
        $entry=Find-Control $Row.title ([System.Windows.Automation.ControlType]::Edit)
        $visible=$entry -and !$entry.Current.IsOffscreen -and $entry.Current.BoundingRectangle.Height -gt 0
        if([bool]$visible -ne $Open){return $false}
        $value=@{row=(Swatch-Bounds $Frames.row);strip=(Swatch-Bounds $Frames.strip);entry=$null;entry_in_strip=$false}
        if($Open){$value.entry=Swatch-Bounds $entry;$value.entry_in_strip=$null -ne $Frames.strip.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,"setting-text-$Id"))}
        if($value.row.height -le 0 -or $value.strip.height -le 0 -or $Frames.strip.Current.IsOffscreen){return $false}
        $key=$value|ConvertTo-Json -Depth 5 -Compress
        if($key -ne $stable.key){$stable.key=$key;$stable.watch.Restart()};$stable.value=$value
        $stable.watch.ElapsedMilliseconds -ge 250
    } "Native $Id allocation did not settle with its editor open=$Open"
    $stable.value
}
function Assert-SwatchAllocation($Closed,$Open,$ClosedAgain,[double]$Gap,[double]$Scale) {
    if([double]::IsNaN($Scale) -or [double]::IsInfinity($Scale) -or $Scale -le 0 -or !$Open.entry -or $Open.entry_in_strip){throw 'Native swatch allocation does not isolate its revealed entry'}
    $inset=$Open.row.bottom-$Open.entry.bottom
    $metrics=[ordered]@{gap=($Open.entry.top-$Open.strip.bottom)/$Scale;growth=($Open.row.height-$Closed.row.height-$Open.entry.height)/$Scale;closed_excess=($Closed.row.bottom-$Closed.strip.bottom-$inset)/$Scale;closed_again_excess=($ClosedAgain.row.bottom-$ClosedAgain.strip.bottom-$inset)/$Scale;closed_height_change=($ClosedAgain.row.height-$Closed.row.height)/$Scale;strip_height_change=($Open.strip.height-$Closed.strip.height)/$Scale;row_width_change=($Open.row.width-$Closed.row.width)/$Scale}
    foreach($name in $metrics.Keys){$expected=if($name -in @('gap','growth')){$Gap}else{0};if([double]::IsNaN($metrics[$name]) -or [double]::IsInfinity($metrics[$name]) -or [Math]::Abs($metrics[$name]-$expected) -gt 1./$Scale){throw "Native custom-color allocation differs at ${name}: $($metrics|ConvertTo-Json -Compress)"}}
    $metrics
}
function Check-SwatchAllocation([string]$Id) {
    $row=Swatch-Row $Id;if(!$row -or $row.kind.type -ne 'swatches'){throw "Shared swatch row is missing for $Id"}
    $saved=$row.kind.value;$selected=[int]$row.kind.selected;$custom=$row.kind.swatches.Count-1
    if(!$row.kind.swatches[$custom].custom){throw 'Shared custom swatch is not the final native choice'}
    $record=[ordered]@{theme=(Read-Model).state.theme;id=$Id;scale=([CapyRowPointer]::GetDpiForWindow($app.MainWindowHandle)/96.);closed=$null;open=$null;closed_again=$null;metrics=$null}
    $script:swatchAllocations.Add($record)
    try{
        $preset=Swatch-Button $Id 0;$scroll=$null;if($preset.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}
        $preset.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $frames=Swatch-Frames $Id $row;$record.row_runtime_id=$frames.row.GetRuntimeId()-join ':';$record.strip_runtime_id=$frames.strip.GetRuntimeId()-join ':'
        $record.closed=Wait-SwatchAllocation $Id $row $frames $false
        (Swatch-Button $Id $custom).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $record.open=Wait-SwatchAllocation $Id $row $frames $true
        if(!(Control $row.title ([System.Windows.Automation.ControlType]::Edit)).Current.HasKeyboardFocus){throw 'Opening Custom did not focus the retained native field'}
        (Swatch-Button $Id 0).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $record.closed_again=Wait-SwatchAllocation $Id $row $frames $false
        $gap=if($row.kind.inline){8.}else{12.};$record.metrics=Assert-SwatchAllocation $record.closed $record.open $record.closed_again $gap $record.scale
    }finally{
        try{
            (Swatch-Button $Id $selected).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
            if($row.kind.swatches[$selected].custom){Edit-Text $row.title $saved;Focus-Control (Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox))}
            Wait-Until {(Swatch-Row $Id).kind.value -eq $saved} "Native $Id swatch did not restore its original shared value"
        }finally{[IO.File]::WriteAllText((Join-Path (Split-Path -Parent $StateFile) 'header-settings-custom-colors.json'),(@($script:swatchAllocations.ToArray())|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))}
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
& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'
if(Find-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)){throw 'View must not include Dark Mode'}
Open-Preferences
$originalTheme=(Read-Model).state.settings.theme
$restoreTheme=if($originalTheme -eq 'dark'){'Dark'}elseif($originalTheme -eq 'light'){'Light'}else{'System'}
$themeIdentity=(Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).GetRuntimeId() -join ':'
foreach($choice in @('Light','Dark',$restoreTheme)){
    [CapyRowPointer]::SetForegroundWindow($app.MainWindowHandle)|Out-Null
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
Wait-Until {Find-Control 'Prediction amount' ([System.Windows.Automation.ControlType]::Button)} 'Shared search results missing'
Invoke-Control 'Prediction amount'
Wait-Until {Find-Control 'Prediction amount slider' ([System.Windows.Automation.ControlType]::Slider)} 'Search result did not reveal the prediction slider'
$feedback=(Read-Model).state.settings.feedback
Toggle-Control 'Enable stroke prediction'
Wait-Until {(Read-Model).state.settings.feedback -ne $feedback} 'Preview toggle did not reach shared settings'
Wait-Until {(Control 'Prediction amount slider' ([System.Windows.Automation.ControlType]::Slider)).Current.IsEnabled -eq !$feedback} 'Prediction slider has the wrong enabled state'
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
Invoke-Control 'Close'
Wait-Until {!(Read-Model).preferences.shortcut_editor} 'Close did not dismiss the nested shortcut editor'
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
