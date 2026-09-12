param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected a controlled CapyCanvas review process.'}
$watch=[Diagnostics.Stopwatch]::StartNew()
do {$app.Refresh();if($app.HasExited){throw 'Review app exited'};if($app.MainWindowHandle -ne [IntPtr]::Zero){break};Start-Sleep -Milliseconds 100} while($watch.Elapsed.TotalSeconds -lt 30)
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
$script:settingsScope=$root
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
function Open-Preferences {
    $script:settingsScope=$root
    Invoke-Control 'Preferences'
    $script:settingsScope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
}
function Close-Preferences {
    Invoke-Control 'Close'
    $script:settingsScope=$root
    Wait-Until {!(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
Wait-Until {Read-Model} 'Launch this review instance with CAPY_TRACE_UI=1 and pass its ui-state.json file' 30
Wait-Until {(Read-Model).brush_ready} 'Shared brush startup did not finish before interaction checks' 45
if(!(Read-Model).windows_isolated_settings){throw 'Launch this fixture with CAPY_SETTINGS_DIRECTORY pointing to a disposable profile.'}
if(Find-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)){throw 'Close Preferences before running this fixture.'}
& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'View'
if(Find-Control 'Dark Mode' ([System.Windows.Automation.ControlType]::MenuItem)){throw 'View must not include Dark Mode'}
Open-Preferences
$originalTheme=(Read-Model).state.settings.theme
$restoreTheme=if($originalTheme -eq 'dark'){'Dark'}elseif($originalTheme -eq 'light'){'Light'}else{'System'}
foreach($choice in @('Light','Dark',$restoreTheme)){
    (Control 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $script:settingsScope=$root
    (Control $choice ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Read-Model).state.settings.theme -eq $(if($choice -eq 'System'){$null}else{$choice.ToLowerInvariant()})} 'Color theme preference did not update'
    $script:settingsScope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
}
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

function Status-Control([string]$Id){
    $entry=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
    if($entry -and !$entry.Current.IsOffscreen){$entry}
}
function Set-ClockPreference([string]$Policy){
    Open-Preferences
    Invoke-Control 'Appearance'
    $picker=Control 'Show battery and clock' ([System.Windows.Automation.ControlType]::ComboBox)
    $picker.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $script:settingsScope=$root
    $choice=@{always='Always';never='Never';fullscreen='In fullscreen mode'}[$Policy]
    (Control $choice ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Read-Model).state.settings.show_clock -eq $Policy} 'Shared status preference did not change'
    $script:settingsScope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
    Close-Preferences
}
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyHeaderPower {
    [StructLayout(LayoutKind.Sequential)] public struct Status {public byte ac,flags,percent,saver; public uint remaining,full;}
    [DllImport("kernel32.dll")] public static extern bool GetSystemPowerStatus(out Status status);
}
'@
$originalClock=(Read-Model).state.settings.show_clock
Set-ClockPreference 'always'
Wait-Until {(Status-Control 'system-clock').Current.Name -match '\d.*\d'} 'Always-visible clock did not appear'
Wait-Until {
    $power=[CapyHeaderPower+Status]::new()
    $known=[CapyHeaderPower]::GetSystemPowerStatus([ref]$power) -and !($power.flags -band 128) -and $power.percent -le 100
    $battery=Status-Control 'system-battery'
    if(!$known){return !$battery}
    $expected='Battery '+$power.percent+'%'+$(if($power.flags -band 8){', charging'}elseif($power.percent -le 15){', low'}else{''})
    $battery -and $battery.Current.Name -eq $expected
} 'Native battery observation does not match Windows power status' 20
Set-ClockPreference 'never'
Wait-Until {!(Status-Control 'system-clock') -and !(Status-Control 'system-battery')} 'Never preference left status visible'
Set-ClockPreference 'fullscreen'
Wait-Until {!(Status-Control 'system-clock')} 'Fullscreen-only clock appeared in a normal window'
Invoke-Control 'Full screen'
Wait-Until {Find-Control 'Exit full screen' ([System.Windows.Automation.ControlType]::Button)} 'Fullscreen presenter did not update header'
Wait-Until {Status-Control 'system-clock'} 'Fullscreen clock did not appear'
Invoke-Control 'Exit full screen'
Wait-Until {Find-Control 'Full screen' ([System.Windows.Automation.ControlType]::Button)} 'Windowed presenter did not restore header'
Wait-Until {!(Status-Control 'system-clock')} 'Fullscreen clock remained after returning to a normal window'
if($originalClock -ne 'fullscreen'){Set-ClockPreference $originalClock}
[pscustomobject]@{
    view_menu_without_theme='passed'
    preferences_theme_roundtrip='passed'
    settings_color_validation='passed'
    retained_settings_field='passed'
    exclusive_icon_tiles='passed'
    shared_settings_search='passed'
    dependent_settings_controls='passed'
    dialog_reopen_roundtrip='passed'
    shortcut_editor_cancel='passed'
    fullscreen_roundtrip='passed'
    clock_and_battery_visibility_policy='passed'
    native_power_observation='passed'
    input_method='native UI Automation; not OS pointer or keyboard delivery'
} | ConvertTo-Json
