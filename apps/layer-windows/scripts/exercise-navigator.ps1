param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyNavigatorCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/navigator/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {$snapshot=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($snapshot.process_id -eq $review.Id){$snapshot.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=5) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Navigator review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Id,$Type){
    $property=if($Id){[System.Windows.Automation.AutomationElement]::AutomationIdProperty}else{[System.Windows.Automation.AutomationElement]::NameProperty}
    $match=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$match=[System.Windows.Automation.AndCondition]::new($match,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$match)
}
function Control([string]$Value,[switch]$Id,$Type){
    $hit=@{element=$null}
    Wait-Until {$hit.element=Find $Value -Id:$Id -Type $Type;$null -ne $hit.element} "Missing control: $Value"
    $hit.element
}
function Invoke([string]$Value,[switch]$Id){(Control $Value -Id:$Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Navigator {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    $menuCondition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Navigator panel'),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    Wait-Until {$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuCondition)} 'Navigator menu item is missing'
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$menuCondition).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
}
function Capture([string]$Name){
    Start-Sleep -Milliseconds 250 # Let the acknowledged native layout reach composition.
    $rect=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($review.MainWindowHandle,[ref]$rect)){throw 'Window bounds unavailable'}
    $bitmap=[Drawing.Bitmap]::new($rect.right-$rect.left,$rect.bottom-$rect.top)
    $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
    try {if(![CapyNavigatorCapture]::PrintWindow($review.MainWindowHandle,$dc,2)){throw 'App-only capture failed'}}
    finally {$graphics.ReleaseHdc($dc);$graphics.Dispose()}
    try {$bitmap.Save((Join-Path $run ($Name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}catch{$bitmap.Dispose();throw}
    return $bitmap
}
function Image-Rect {
    $overview=(Control 'navigator-overview' -Id).Current.BoundingRectangle
    $window=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $scale=[CapyNavigatorCapture]::GetDpiForWindow($review.MainWindowHandle)/96.
    $document=(Model).state.tabs[0]
    $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
    $width=$document.width*$fit;$height=$document.height*$fit
    # Stay inside the transparent cutout, away from antialiased boundary pixels.
    [Drawing.Rectangle]::FromLTRB(
        [int][Math]::Ceiling($overview.Left-$window.left+($overview.Width-$width)/2+2),
        [int][Math]::Ceiling($overview.Top-$window.top+($overview.Height-$height)/2+2),
        [int][Math]::Floor($overview.Left-$window.left+($overview.Width+$width)/2-2),
        [int][Math]::Floor($overview.Top-$window.top+($overview.Height+$height)/2-2))
}
function Check-Surround($Capture) {
    $overview=(Control 'navigator-overview' -Id).Current.BoundingRectangle
    $window=[CapyNavigatorCapture+Rect]::new()
    if(![CapyNavigatorCapture]::GetWindowRect($review.MainWindowHandle,[ref]$window)){throw 'Window bounds unavailable'}
    $pixel=$Capture.GetPixel([int]($overview.Left-$window.left+2),[int]($overview.Top-$window.top+2))
    $expected=[Drawing.ColorTranslator]::FromHtml((Model).state.palette.bg)
    if($pixel.R -ne $expected.R -or $pixel.G -ne $expected.G -or $pixel.B -ne $expected.B){throw 'Navigator surround differs from the shared palette'}
}
function Different($Before,$After,$Rect) {
    $count=0
    for($y=$Rect.Top;$y -lt $Rect.Bottom;$y++){for($x=$Rect.Left;$x -lt $Rect.Right;$x++){
        $a=$Before.GetPixel($x,$y);$b=$After.GetPixel($x,$y)
        if([Math]::Abs([int]$a.R-$b.R)+[Math]::Abs([int]$a.G-$b.G)+[Math]::Abs([int]$a.B-$b.B) -gt 30){$count++}
    }}
    $count
}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/navigator-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned Navigator review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    if(!(Model).windows_isolated_settings){throw 'Navigator fixture requires an isolated profile'}
    [CapyNavigatorCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    Navigator
    $overview=Control 'navigator-overview' -Id
    $identity=$overview.GetRuntimeId() -join ':'
    $scale=[CapyNavigatorCapture]::GetDpiForWindow($review.MainWindowHandle)/96.
    if($overview.Current.BoundingRectangle.Height -gt 220*$scale+1){throw 'Overview exceeds the shared 220-unit height cap'}
    foreach($id in @('zoom_out','zoom_in','rotate_left','rotate_right','flip_horizontal','flip_vertical')){
        $button=Control ('navigator-'+$id) -Id
        $command=(Model).state.commands|Where-Object {$_.id -eq $id}
        if($button.Current.Name -ne $command.label -or $button.Current.IsEnabled -ne $command.enabled){throw "Navigator command projection differs: $id"}
    }
    $camera=(Control 'canvas-camera' -Id).Current.Name
    Invoke 'navigator-zoom_in' -Id
    Wait-Until {(Control 'canvas-camera' -Id).Current.Name -ne $camera} 'Zoom did not update the native camera readout'
    Invoke 'navigator-zoom_out' -Id
    Wait-Until {(Control 'canvas-camera' -Id).Current.Name -eq $camera} 'Inverse zoom did not restore the view'
    Invoke 'navigator-rotate_left' -Id
    Wait-Until {(Control 'canvas-camera' -Id).Current.Name -ne $camera} 'Rotate did not update the view'
    Invoke 'navigator-rotate_right' -Id
    Wait-Until {(Control 'canvas-camera' -Id).Current.Name -eq $camera} 'Inverse rotation did not restore the view'
    foreach($id in @('flip_horizontal','flip_vertical')){
        Invoke ('navigator-'+$id) -Id
        Wait-Until {(Control ('navigator-'+$id) -Id).Current.ItemStatus -eq 'Selected'} 'Flip did not become selected'
        Invoke ('navigator-'+$id) -Id
        Wait-Until {(Control ('navigator-'+$id) -Id).Current.ItemStatus -eq ''} 'Flip did not clear'
    }
    if((Model).state.document_file.modified){throw 'Camera controls modified the document'}
    if(((Control 'navigator-overview' -Id).GetRuntimeId() -join ':') -ne $identity){throw 'Camera updates replaced the native overview'}
    $area=Image-Rect
    $blank=Capture 'blank'
    try {
        Check-Surround $blank
        Invoke 'Test stroke'
        Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not reach the shared document'
        $paint=Capture 'paint'
        try {$changed=Different $blank $paint $area;if($changed -lt 20){throw "Live GPU overview did not show the stroke ($changed changed pixels)"}}finally{$paint.Dispose()}
        Invoke 'Undo'
        Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore the checkpoint'
        $undo=Capture 'undo'
        try {if((Different $blank $undo $area) -ne 0){throw 'Undo did not restore the GPU overview pixels'}}finally{$undo.Dispose()}
    }finally{$blank.Dispose()}
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File';Invoke 'new_document' -Id
    (Control 'document-width' -Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('128')
    (Control 'document-height' -Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
    Invoke 'Create'
    Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 64 -and !(Model).state.document_file.busy} 'Different-aspect document was not adopted' 45
    Wait-Until {(Control 'Drawing canvas').Current.IsEnabled} 'Document dialog input gate did not clear'
    if(((Control 'navigator-overview' -Id).GetRuntimeId() -join ':') -ne $identity){throw 'Document replacement rebuilt the Navigator'}
    $replacement=Capture 'replacement'
    try {
        Check-Surround $replacement
        $area=Image-Rect
        $inside=$replacement.GetPixel($area.Left+8,$area.Top+8)
        if($inside.R -lt 250 -or $inside.G -lt 250 -or $inside.B -lt 250){throw 'New document did not fill the expected overview image'}
    }finally{$replacement.Dispose()}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1550 -Height 1040
    Wait-Until {(Model).state.camera.viewport[0] -gt 1450} 'Native resize did not reach the renderer'
    if(((Control 'navigator-overview' -Id).GetRuntimeId() -join ':') -ne $identity){throw 'Resize replaced the native Navigator'}
    Navigator
    Wait-Until {!(Find 'navigator-overview' -Id)} 'Hiding Navigator left its native controls loaded'
    Navigator
    $null=Control 'navigator-overview' -Id
    $theme=(Model).state.theme
    Invoke 'Preferences'
    (Control 'Color theme' -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=if($theme -eq 'dark'){'Light'}else{'Dark'}
    (Control $choice -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $theme} 'Theme change not acknowledged'
    Invoke 'Close'
    Wait-Until {!(Find 'Preferences' -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
    $null=Control 'navigator-overview' -Id
    $light=Capture 'alternate-theme'
    try{Check-Surround $light}finally{$light.Dispose()}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native runtime stderr requires inspection'}
    [PSCustomObject]@{
        shared_commands_and_camera='passed';retained_controls_and_resize='passed'
        live_stroke_and_exact_undo='passed';changed_preview_pixels=$changed
        document_aspect_replacement='passed';hide_reopen_and_theme='passed';zero_exit='passed'
        scope='isolated native controls and app-only GPU pixels; physical pointer gestures, full-editor parity and presentation acceptance remain separate'
    }|ConvertTo-Json
}catch{[IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw}finally{foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
