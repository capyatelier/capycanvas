param([Parameter(Mandatory)][int]$ProcessId)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$process=Get-Process -Id $ProcessId
$root=[System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
function Find-Control([string]$Name,$Type) {
    $nameCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name)
    $typeCondition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type)
    $result=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new($nameCondition,$typeCondition))
    if(!$result){throw "Missing control: $Name"}
    return $result
}
function Invoke-Control([string]$Name) {
    (Find-Control $Name ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Read-Size {
    (Find-Control 'Brush size' ([System.Windows.Automation.ControlType]::Edit)).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
}
function Wait-Size([string]$Expected) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        $value=Read-Size
        if($value -eq $Expected){return}
        Start-Sleep -Milliseconds 50
    } while($watch.Elapsed.TotalSeconds -lt 5)
    throw "Brush size should be $Expected, got $value"
}
function Count-Layers {
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Layer visibility')
    $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition).Count
}
$entry=Find-Control 'Brush size' ([System.Windows.Automation.ControlType]::Edit)
$originalId=$entry.GetRuntimeId() -join ':'
$slider=Find-Control 'Brush size slider' ([System.Windows.Automation.ControlType]::Slider)
$slider.GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
Wait-Size '32.0 px'
$entry=Find-Control 'Brush size' ([System.Windows.Automation.ControlType]::Edit)
if(($entry.GetRuntimeId() -join ':') -ne $originalId){throw 'Value update replaced the native text field'}
Invoke-Control '64 px'
Wait-Size '64.0 px'
$before=Count-Layers
Invoke-Control 'New layer'
$watch=[Diagnostics.Stopwatch]::StartNew()
do {Start-Sleep -Milliseconds 50;$after=Count-Layers} while($after -ne ($before+1) -and $watch.Elapsed.TotalSeconds -lt 5)
if($after -ne $before+1){throw 'New layer did not appear in the shared workspace'}
Invoke-Control 'Undo'
$watch.Restart()
do {Start-Sleep -Milliseconds 50;$after=Count-Layers} while($after -ne $before -and $watch.Elapsed.TotalSeconds -lt 5)
if($after -ne $before){throw 'Undo did not restore the layer list'}
[pscustomobject]@{
    logarithmic_slider='passed'
    size_preset='passed'
    retained_text_field='passed'
    new_layer_and_undo='passed'
    input_method='native UI Automation; not OS pointer delivery'
} | ConvertTo-Json
