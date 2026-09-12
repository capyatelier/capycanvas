param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile,
      [Parameter(Mandatory)][string]$Python)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyColorCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
[CapyColorCapture]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run=Join-Path $repo ('artifacts/windows/color/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected a controlled CapyCanvas review process'}
function Model {
    try {$snapshot=Get-Content -LiteralPath $StateFile -Raw | ConvertFrom-Json;if($snapshot.process_id -eq $ProcessId){return $snapshot.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};Start-Sleep -Milliseconds 75}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review startup did not complete' 45
if(!(Model).windows_isolated_settings){throw 'Use an isolated review settings profile'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
function Find([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button){
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button){
    $script:found=$null
    Wait-Until {$script:found=Find $Name $Type;$null -ne $script:found} "Missing control: $Name"
    $script:found
}
function Invoke-Control([string]$Name){(Control $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Focus($Entry){$Entry.SetFocus();Wait-Until {$Entry.Current.HasKeyboardFocus} 'Native control did not receive focus'}
function Edit([string]$Name,[string]$Value){
    $entry=Control $Name ([System.Windows.Automation.ControlType]::Edit);Focus $entry
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)
    Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $Value} 'Numeric draft was not retained'
}
function Component([string]$Name,[int]$Index,[string]$Expression,[double]$Expected,[string]$Next='Hue'){
    Edit $Name $Expression
    Focus (Control $Next ([System.Windows.Automation.ControlType]::Edit))
    Wait-Until {[Math]::Abs((Model).color_panel.components[$Index].value-$Expected) -lt .01} "Color component did not commit: $Name"
}
function Capture([string]$Name){
    Start-Sleep -Milliseconds 250 # Allow the acknowledged XAML image update to be composed.
    $wheel=(Control 'Color wheel' ([System.Windows.Automation.ControlType]::Image)).Current.BoundingRectangle
    $rect=[CapyColorCapture+Rect]::new()
    if(![CapyColorCapture]::GetWindowRect($app.MainWindowHandle,[ref]$rect)){throw 'Window geometry unavailable'}
    $width=$rect.right-$rect.left;$height=$rect.bottom-$rect.top
    $bitmap=[Drawing.Bitmap]::new($width,$height,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
    try {if(![CapyColorCapture]::PrintWindow($app.MainWindowHandle,$dc,2)){throw 'App-only capture failed'}}
    finally {$graphics.ReleaseHdc($dc)}
    $png=Join-Path $run ($Name+'.png')
    try {$bitmap.Save($png,[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$bitmap.Dispose()}
    $state=Model
    $slot=$state.state.colors.paint_slot
    $fixture=@{viewport=@($width,$height);wheel=@(($wheel.Left-$rect.left),($wheel.Top-$rect.top),$wheel.Width,$wheel.Height);
        space=$state.color_panel.space;rgba=$state.state.colors.$slot;hue=$state.color_panel.components[0].value}
    $json=Join-Path $run ($Name+'.json')
    [IO.File]::WriteAllText($json,($fixture|ConvertTo-Json -Depth 4))
    & $Python (Join-Path $repo 'tools/visual/check_color_wheel.py') $png $json --oracle (Join-Path $repo 'target/debug/examples/color_wheel_reference.exe') --output (Join-Path $run ($Name+'-report.json'))
    if($LASTEXITCODE -ne 0){throw "Color pixel comparison failed: $Name"}
}
& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
(Control 'Color panel' ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
$null=Control 'Color wheel' ([System.Windows.Automation.ControlType]::Image)
$original=(Control 'Hue' ([System.Windows.Automation.ControlType]::Edit)).GetRuntimeId() -join ':'
Component 'Hue' 0 '120 + 30' 150 'Saturation'
Component 'Saturation' 1 '55' 55
Component 'Value' 2 '65' 65
if(((Control 'Hue' ([System.Windows.Automation.ControlType]::Edit)).GetRuntimeId() -join ':') -ne $original){throw 'Color edits replaced the numeric field'}
Capture 'hsv'
$before=(Model).state.colors.foreground
Invoke-Control 'Switch HSV square / HLS triangle'
Wait-Until {(Model).color_panel.space -eq 'hls'} 'HLS mode did not open'
$null=Control 'Lightness' ([System.Windows.Automation.ControlType]::Edit)
if(((Model).state.colors.foreground|ConvertTo-Json -Compress) -ne ($before|ConvertTo-Json -Compress)){throw 'Switching color spaces changed paint'}
Capture 'hls'
# A draft in the old color/space context must not write into the new paint slot.
Edit 'Hue' '270'
Invoke-Control 'Background color'
Wait-Until {(Model).state.colors.paint_slot -eq 'background'} 'Background slot was not selected'
if(((Model).state.colors.foreground|ConvertTo-Json -Compress) -ne ($before|ConvertTo-Json -Compress)){throw 'Changing slots committed a stale draft'}
if(((Model).state.colors.background|ConvertTo-Json -Compress) -ne '[1.0,1.0,1.0,1.0]' -and ((Model).state.colors.background|ConvertTo-Json -Compress) -ne '[1,1,1,1]'){throw 'Changing slots overwrote the background'}
Invoke-Control 'Transparent paint'
Wait-Until {(Model).state.colors.slot -eq 'transparent'} 'Transparent paint was not selected'
Invoke-Control 'Foreground color'
Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground was not restored'
Invoke-Control 'Swap foreground and background'
Wait-Until {((Model).state.colors.background|ConvertTo-Json -Compress) -eq ($before|ConvertTo-Json -Compress)} 'Color swap did not preserve paint'
Invoke-Control 'Swap foreground and background'
Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Compress) -eq ($before|ConvertTo-Json -Compress)} 'Color swap did not restore paint'

function Entries([string]$Name){
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))
    @($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition))
}
Invoke-Control 'Brush color'
Wait-Until {@(Entries 'Hue').Count -eq 2} 'The popup did not project the shared Color panel'
# The roomier popup has the wider numeric field; both instances bind one model.
$popupHue=Entries 'Hue'|Sort-Object {$_.Current.BoundingRectangle.Width} -Descending|Select-Object -First 1
Focus $popupHue
$popupHue.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('210')
Focus (Entries 'Lightness'|Sort-Object {$_.Current.BoundingRectangle.Width} -Descending|Select-Object -First 1)
Wait-Until {[Math]::Abs((Model).color_panel.components[0].value-210) -lt .01} 'Popup color edit did not reach Rust'
Wait-Until {@(Entries 'Hue'|Where-Object {$_.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne '210'}).Count -eq 0} 'Docked and popup color fields diverged'
Invoke-Control 'Brush color'
Wait-Until {@(Entries 'Hue').Count -eq 1 -and !(Model).state.customization.control} 'Color popup did not close'
Component 'Lightness' 1 '0' 0
Component 'Hue' 0 '269' 269 'Lightness'
Capture 'hls-black'
Invoke-Control 'Switch HSV square / HLS triangle'
Wait-Until {(Model).color_panel.space -eq 'hsv'} 'HSV mode did not reopen'
Capture 'hsv-black'
[pscustomobject]@{popup_and_dock_synchronization='passed';remembered_hue_pixels='passed';hsv_hls_pixels='passed';numeric_expression_and_retention='passed';space_preserves_rgba='passed';
    stale_draft_context='passed';swatches_and_swap='passed';scope='native UI Automation and sampled gradient pixels; physical pointer capture and full-editor parity remain open'}|ConvertTo-Json
