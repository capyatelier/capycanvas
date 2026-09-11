param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected a controlled CapyCanvas review process.'}
$watch=[Diagnostics.Stopwatch]::StartNew()
do {$app.Refresh();if($app.HasExited){throw 'Review app exited'};if($app.MainWindowHandle -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 100} while($watch.Elapsed.TotalSeconds -lt 30)
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
$script:scope=$root
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
    $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new($nameCondition,$typeCondition))
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$TimeoutSeconds=5) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {if(& $Condition){return};Start-Sleep -Milliseconds 75} while($watch.Elapsed.TotalSeconds -lt $TimeoutSeconds)
    throw $Message
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    Wait-Until {Find-Control $Name $Type} "Missing control: $Name"
    Find-Control $Name $Type
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
function Open-Preferences {
    $script:scope=$root
    Invoke-Control 'Preferences'
    Wait-Until {Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)} 'Preferences did not become visible after an open request'
    $script:scope=Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
}
function Close-Preferences {
    Invoke-Control 'Close'
    $script:scope=$root
    Wait-Until {!(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
Wait-Until {Read-Model} 'Launch this review instance with CAPY_TRACE_UI=1 and pass its ui-state.json file' 30
Wait-Until {(Read-Model).brush_ready} 'Shared brush startup did not finish before interaction checks' 45
if(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)){throw 'Close Preferences before running this fixture.'}
Invoke-Control 'View'
$before=Toggle-State 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)
Toggle-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)
Wait-Until {!(Find-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem))} 'Menu did not close'
Invoke-Control 'View'
if((Toggle-State 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)) -eq $before){throw 'Shared theme state did not change'}
Toggle-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)
Wait-Until {!(Find-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem))} 'Menu did not close'

Open-Preferences
$base=Read-Text 'Dark theme base color'
$entry=Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)
$identity=$entry.GetRuntimeId() -join ':'
Edit-Text 'Dark theme base color' '#1c2c3c'
Focus-Control (Control 'Light theme base color' ([System.Windows.Automation.ControlType]::Edit))
Wait-Until {(Read-Model).state.settings.dark_base -eq '#1c2c3c'} 'Valid base color did not reach shared settings'
Wait-Until {(Read-Text 'Dark theme base color') -eq '#1c2c3c'} 'Valid base color was not retained'
if(((Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)).GetRuntimeId() -join ':') -ne $identity){throw 'Palette update replaced the native settings field'}
Edit-Text 'Dark theme base color' 'invalid'
Focus-Control (Control 'Light theme base color' ([System.Windows.Automation.ControlType]::Edit))
Wait-Until {(Read-Model).preferences.error} 'Invalid color did not produce shared validation feedback'
Wait-Until {(Read-Text 'Dark theme base color') -eq '#1c2c3c'} 'Invalid base color replaced the valid value'
Edit-Text 'Dark theme base color' $base
Focus-Control (Control 'Light theme base color' ([System.Windows.Automation.ControlType]::Edit))
Wait-Until {(Read-Model).state.settings.dark_base -eq $base} 'Base color did not restore in shared settings'
Wait-Until {(Read-Text 'Dark theme base color') -eq $base} 'Base color restoration failed'

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
Wait-Until {Find-Control 'Prediction time' ([System.Windows.Automation.ControlType]::Button)} 'Shared search results missing'
Invoke-Control 'Prediction time'
Wait-Until {Find-Control 'Prediction time' ([System.Windows.Automation.ControlType]::Edit)} 'Search result did not reveal the numeric preference'
$feedback=(Read-Model).state.settings.feedback
Toggle-Control 'Live stroke preview'
Wait-Until {(Read-Model).state.settings.feedback -ne $feedback} 'Preview toggle did not reach shared settings'
foreach($name in @('Prediction time','Pen tip tracking')){
    Wait-Until {(Control $name ([System.Windows.Automation.ControlType]::Edit)).Current.IsEnabled -eq !$feedback} 'Dependent numeric editor has the wrong enabled state'
}
Toggle-Control 'Live stroke preview'
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
Invoke-Control 'Cancel'
Wait-Until {!(Read-Model).preferences.capture -and (Read-Model).preferences.shortcut_editor} 'Cancel did not return to the shortcut editor'
Invoke-Control 'Close'
Wait-Until {!(Read-Model).preferences.shortcut_editor} 'Close did not dismiss the nested shortcut editor'
Close-Preferences

Invoke-Control 'Full screen'
Wait-Until {Find-Control 'Exit full screen' ([System.Windows.Automation.ControlType]::Button)} 'Fullscreen presenter did not update header'
Invoke-Control 'Exit full screen'
Wait-Until {Find-Control 'Full screen' ([System.Windows.Automation.ControlType]::Button)} 'Windowed presenter did not restore header'
[pscustomobject]@{
    menu_theme_roundtrip='passed'
    settings_color_validation='passed'
    retained_settings_field='passed'
    exclusive_icon_tiles='passed'
    shared_settings_search='passed'
    dependent_settings_controls='passed'
    dialog_reopen_roundtrip='passed'
    shortcut_editor_cancel='passed'
    fullscreen_roundtrip='passed'
    input_method='native UI Automation; not OS pointer or keyboard delivery'
} | ConvertTo-Json
