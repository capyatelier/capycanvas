param(
    [Parameter(Mandatory)][int]$ProcessId,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [int]$Width=960,[int]$Height=660
)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyEditorCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
    [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window,int x,int y,int width,int height,bool repaint);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
[CapyEditorCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$review=Get-Process -Id $ProcessId
$null=$review.Handle
if($review.ProcessName -ne 'CapyCanvas'){throw 'Expected the controlled native editor'}
$statePath=Join-Path (Split-Path -Parent $review.Path) 'ui-state.json'
$startup=[Diagnostics.Stopwatch]::StartNew()
do{
    $review.Refresh();if($review.HasExited){throw 'The native editor exited before creating its window'}
    $handle=$review.MainWindowHandle
    if($handle){break}
    Start-Sleep -Milliseconds 100
}while($startup.Elapsed.TotalSeconds -lt 45)
if(!$handle){throw 'The native editor did not create a window'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
$OutputDirectory=[IO.Path]::GetFullPath($OutputDirectory)
[IO.Directory]::CreateDirectory($OutputDirectory)|Out-Null
function Model {
    try{
        $s=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
        if($s.process_id -ne $ProcessId){return}
        $camera=Get-Content -LiteralPath (Join-Path (Split-Path -Parent $statePath) 'camera-state.json') -Raw|ConvertFrom-Json
        if($camera.process_id -ne $ProcessId -or $camera.window_id -ne $s.window_id){return}
        if($camera.camera.revision -ge $s.model.state.camera.revision){$s.model.state.camera=$camera.camera}
        $s.model
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=45) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        if(& $Condition){return}
        $review.Refresh();if($review.HasExited){throw 'Owned editor exited during capture'}
        Start-Sleep -Milliseconds 100
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button,$Scope=$root) {
    $Scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type)))
}
function Invoke([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button,$Scope=$root) {
    Wait-Until {Find $Name $Type $Scope} "Missing native control: $Name"
    (Find $Name $Type $Scope).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Fit-Canvas {
    Invoke 'View'
    Invoke 'Fit canvas' ([System.Windows.Automation.ControlType]::MenuItem)
}
function Set-Theme([string]$Theme) {
    Invoke 'Preferences'
    Wait-Until {Find 'Preferences' ([System.Windows.Automation.ControlType]::Window)} 'Preferences did not open'
    $dialog=Find 'Preferences' ([System.Windows.Automation.ControlType]::Window)
    $picker=Find 'Color theme' ([System.Windows.Automation.ControlType]::ComboBox) $dialog
    $picker.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Wait-Until {Find $Theme ([System.Windows.Automation.ControlType]::ListItem)} 'Theme choices did not open'
    (Find $Theme ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq $Theme.ToLowerInvariant()} 'Theme did not reach shared state'
    Invoke 'Close' ([System.Windows.Automation.ControlType]::Button) $dialog
    Wait-Until {!(Find 'Preferences' ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
function Settle {
    $script:previousGeometry=$null;$script:stable=0
    Wait-Until {
        $m=Model
        if(!$m -or !$m.brush_ready -or !$m.windows_workspace.ready -or $m.windows_workspace.busy -or $m.windows_filter_load.pending){return $false}
        if($m.error -or $m.state.host_error -or $m.windows_filter_load.error){throw 'Native editor reports an error'}
        $status=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status'))
        if($status -and !$status.Current.IsOffscreen){return $false}
        $readout=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-camera'))
        $expected=([Math]::Round($m.state.camera.zoom*100,[MidpointRounding]::AwayFromZero)).ToString()+'% · 0°'
        if(!$readout -or $readout.Current.Name -ne $expected){return $false}
        $geometry=@($m.layout,$m.panel_measurements,$m.state.camera,$m.titlebar_insets,$m.state.theme)|ConvertTo-Json -Depth 60 -Compress
        if($geometry -eq $script:previousGeometry){$script:stable++}else{$script:stable=0;$script:previousGeometry=$geometry}
        $script:stable -ge 3
    } 'Native editor geometry or startup did not settle'
}
Wait-Until {(Model).windows_workspace.ready -and (Model).brush_ready} 'Native editor did not become ready'
if(!(Model).windows_isolated_settings){throw 'Capture requires an isolated profile and CAPY_TRACE_UI=1'}
if((Model).state.document_file.modified){throw 'Capture starts from an unmodified isolated document'}
$scale=[CapyEditorCapture]::GetDpiForWindow($handle)/96.0
$pixelWidth=$Width*$scale;$pixelHeight=$Height*$scale
if($pixelWidth -ne [Math]::Round($pixelWidth) -or $pixelHeight -ne [Math]::Round($pixelHeight)){throw 'Choose a viewport with integral physical dimensions'}
$client=[CapyEditorCapture+Rect]::new();$outer=[CapyEditorCapture+Rect]::new()
if(![CapyEditorCapture]::GetClientRect($handle,[ref]$client) -or ![CapyEditorCapture]::GetWindowRect($handle,[ref]$outer)){throw 'Cannot measure native client'}
$surface=Find 'Drawing canvas' ([System.Windows.Automation.ControlType]::Custom)
if(!$surface){
    $surface=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Drawing canvas'))
}
$surfaceRect=$surface.Current.BoundingRectangle
if(![CapyEditorCapture]::MoveWindow($handle,$outer.left,$outer.top,
    [int]($outer.right-$outer.left+$pixelWidth-$surfaceRect.Width),
    [int]($outer.bottom-$outer.top+$pixelHeight-$surfaceRect.Height),$true)){throw 'Cannot size native drawing surface'}
Wait-Until {
    [CapyEditorCapture]::GetClientRect($handle,[ref]$client)|Out-Null
    $surface.Current.BoundingRectangle.Width -eq $pixelWidth -and $surface.Current.BoundingRectangle.Height -eq $pixelHeight -and
        (Model).state.camera.viewport[0] -eq $pixelWidth -and (Model).state.camera.viewport[1] -eq $pixelHeight
} 'Native drawing surface and GPU viewport did not reach the exact capture dimensions'
$fixtures=@()
foreach($theme in @('dark','light')){
    Set-Theme ((Get-Culture).TextInfo.ToTitleCase($theme))
    foreach($scenario in @('initial','canvas-under-header')){
        Fit-Canvas
        Settle
        if($scenario -eq 'canvas-under-header'){
            for($i=0;$i -lt 4;$i++){
                $previousZoom=(Model).state.camera.zoom
                Invoke 'Zoom in'
                Wait-Until {(Model).state.camera.zoom -gt $previousZoom} 'Zoom command did not reach the shared camera'
            }
        }
        $surface.SetFocus()
        Settle
        $model=Model
        $name="$theme-$scenario"
        $origin=[CapyEditorCapture+Point]::new()
        if(![CapyEditorCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot locate native client'}
        [CapyEditorCapture]::GetClientRect($handle,[ref]$client)|Out-Null
        $surfaceRect=$surface.Current.BoundingRectangle
        $offset=@([int]($surfaceRect.X-$origin.x),[int]($surfaceRect.Y-$origin.y))
        # Keep the raw full client alongside the complete XAML surface capture.
        # WinUI retains a one-physical-pixel OS frame above its content. This is
        # an explicit capture boundary; every application/titlebar pixel remains.
        $bitmap=[Drawing.Bitmap]::new($client.right,$client.bottom,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
        $graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
        try{if(![CapyEditorCapture]::PrintWindow($handle,$dc,3)){throw 'Native editor capture failed'}}
        finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
        try{
            $bitmap.Save((Join-Path $OutputDirectory "client-$name.png"),[Drawing.Imaging.ImageFormat]::Png)
            # PrintWindow resets the HDC origin, so select the measured XAML
            # rectangle from this same raw frame. Keep the source and offset in
            # the manifest; exclude only OS-owned pixels outside the app surface.
            $bounds=[Drawing.Rectangle]::new($offset[0],$offset[1],[int]$pixelWidth,[int]$pixelHeight)
            $content=$bitmap.Clone($bounds,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
            try{$content.Save((Join-Path $OutputDirectory "native-$name.png"),[Drawing.Imaging.ImageFormat]::Png)}finally{$content.Dispose()}
        }finally{$bitmap.Dispose()}
        $origin.x=[int]$surfaceRect.X;$origin.y=[int]$surfaceRect.Y
        $elements=@()
        foreach($node in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)){
            $entry=$node.Current;$rect=$entry.BoundingRectangle
            if($entry.IsOffscreen -or $rect.IsEmpty -or $rect.Width -le 0 -or $rect.Height -le 0){continue}
            $elements+=@{id=$entry.AutomationId;name=$entry.Name;type=$entry.ControlType.ProgrammaticName;
                bounds=@{x=($rect.X-$origin.x)/$scale;y=($rect.Y-$origin.y)/$scale;width=$rect.Width/$scale;height=$rect.Height/$scale}}
        }
        $model|ConvertTo-Json -Depth 100|Set-Content -LiteralPath (Join-Path $OutputDirectory "model-$name.json")
        $elements|ConvertTo-Json -Depth 6|Set-Content -LiteralPath (Join-Path $OutputDirectory "elements-$name.json")
        $fixtures+=@{name=$name;viewport=@($Width,$Height);scale=$scale;theme=$theme;scenario=$scenario;
            native="native-$name.png";full_client="client-$name.png";surface_offset_pixels=$offset;client_pixels=@($client.right,$client.bottom);
            workspace=$model.windows_workspace.id;titlebar_insets=$model.titlebar_insets;
            document=$model.state.tabs[0];camera=$model.state.camera;layout=$model.layout;
            tool_set=@($elements|Where-Object{$_.id -match '^tool-(group|subtool)-'})}
        Write-Output "Captured $name at $Width x $Height logical, scale $scale"
    }
}
@{schema=1;platform='windows';fixtures=$fixtures;scope='complete XAML drawing surface including custom titlebar; raw client also retained; isolated UI and synthetic commands, not physical input or presentation timing'}|
    ConvertTo-Json -Depth 100|Set-Content -LiteralPath (Join-Path $OutputDirectory 'fixtures.json')
