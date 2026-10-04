param([Parameter(Mandatory)][string]$Executable,[switch]$RecoverGpu,[switch]$FailGpu)
if($RecoverGpu -and $FailGpu){throw "Choose successful recovery or exhausted recovery, not both"}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$CapyCacheModel=$true
$CapyPopups=$true
$drawIndex=0
Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyDocumentControls {
    [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect rect);
    [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h,ref Point point);
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
    [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,StringBuilder text,uint flags,uint timeout,out UIntPtr result);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
    public static void TypeText(IntPtr edit,string text) {
        UIntPtr result;
        if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
        if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
        foreach(char c in text)
            if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
        var actual=new StringBuilder(32768);if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot verify native picker filename");
        if(actual.ToString()!=text)throw new Exception("The native picker filename did not match the test path");
    }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/document-ui/'+[Guid]::NewGuid().ToString('N'))
$settingsProfile=Join-Path $run 'profile'
[IO.Directory]::CreateDirectory($settingsProfile)|Out-Null
$first=Join-Path $run 'Drawing one 日本語.capy'
$second=Join-Path $run 'Drawing two.capy'
$corrupt=Join-Path $run 'Corrupt.capy'
[IO.File]::WriteAllText($corrupt,'synthetic invalid project')
$imageSource=Join-Path $run 'Source image 日本語.png'
$bitmap=[Drawing.Bitmap]::new(16,12,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{
    for($y=0;$y -lt 12;$y++){for($x=0;$x -lt 16;$x++){
        $bitmap.SetPixel($x,$y,[Drawing.Color]::FromArgb(180,210,45,83))
    }}
    $bitmap.Save($imageSource,[Drawing.Imaging.ImageFormat]::Png)
}finally{$bitmap.Dispose()}
try {
Enter-CapyEnvironment @('CAPY_TEST_GPU_UNAVAILABLE')
$env:CAPY_STORAGE_DIR=$settingsProfile
if($FailGpu){$env:CAPY_TEST_GPU_UNAVAILABLE='1'}
$env:CAPY_TRACE_UI='1'
$env:CAPY_SMOKE_TEST='1'
$env:CAPY_TEST_DISPLAY='1'
$env:CAPY_TEST_PRIMARY='1'
$stderr=Join-Path $run 'stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Document review process $($review.Id)"
function Request-Close([switch]$WithPreferences) {
    # Shared completion precedes native dialog teardown. RequestClose ignores
    # window close while a document dialog is open; wait for native readiness.
    # The Preferences case intentionally verifies committing its focused draft.
    if(!$WithPreferences){
        Wait-Until {
            $canvas=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'drawing-canvas'))
            $canvas -and $canvas.Current.IsEnabled
        } 'Native document dialog did not finish closing'
    }
    # Send to the known application owner and retain its process guard.
    $handle=[IntPtr]$root.Current.NativeWindowHandle
    $owner=[uint32]0;[CapyDocumentControls]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Close target does not belong to this review'}
    Wait-Until {[CapyDocumentControls]::IsWindowEnabled($handle)} 'Native owner remained disabled after picker completion'
    if(![CapyDocumentControls]::PostMessage($handle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Native owner rejected the close request'}
}
function Wait-Closed([string]$Message) {
    $handle=$review.MainWindowHandle
    $watch=[Diagnostics.Stopwatch]::StartNew()
    Wait-Until {
        $review.Refresh();if($review.HasExited){return $true}
        if([CapyDocumentControls]::IsWindow($handle)){
            $reply=[UIntPtr]::Zero
            if([CapyDocumentControls]::SendMessageTimeout($handle,0,[UIntPtr]::Zero,[IntPtr]::Zero,2,500,[ref]$reply) -eq [IntPtr]::Zero){
                $review.Refresh()
                if(!$review.HasExited -and [CapyDocumentControls]::IsWindow($handle)){throw 'Native window stopped answering during close'}
            }
        }
        return $false
    } $Message -Closing
    Write-Output ('Native close completed in {0:N2}s' -f $watch.Elapsed.TotalSeconds)
    if($review.ExitCode -ne 0){throw ("Native review exit code: 0x{0:X8}" -f [uint32]($review.ExitCode -band 0xffffffffL))}
}
function Find-Name([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Find-Id([string]$Id) {
    Find $Id -Within $scope
}
function Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
    $script:found=$null;Wait-Until {$script:found=Find-Name $Name $Type;$null -ne $script:found} "Missing control: $Name";$script:found
}
function Invoke-Control([string]$Name) {
    $before=(Model).state.document_file.revision
    Wait-Until {
        try {$item=Find-Name $Name;if(!$item -or !$item.Current.IsEnabled){return $false};$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();$true}
        catch [System.Windows.Automation.ElementNotAvailableException] {(Model).state.document_file.revision -ne $before}
    } "Could not invoke control: $Name"
}
function File-Command([string]$Id,[scriptblock]$Options) {
    $script:scope=$root
    Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Document command stayed disabled: $Id" 45
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
    Wait-Until {$item=Find-Id $Id;$item -and $item.Current.IsEnabled} "Enabled file command not found: $Id"
    (Find-Id $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    if($Id -eq 'export_document') {
        Wait-Until {(Model).windows_document.stage -eq 'options'} 'Export options did not open' 45
        if($Options){& $Options}
        Invoke-Control 'Preview Output'
        Wait-Until {(Model).windows_document.stage -eq 'preview'} 'Export preview did not finish' 60
        Invoke-Control 'Export'
    }
}
function New-Dialog {
    $script:scope=$root
    $script:scope=Control 'New drawing' ([System.Windows.Automation.ControlType]::Window)
}
function Set-Size([string]$Width,[string]$Height) {
    (Find-Id 'document-width').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Width)
    (Find-Id 'document-height').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Height)
}
function Idle {Wait-Until {$current=Model;$current -and !$current.state.document_file.busy} 'Document request did not finish' 45}
function Confirm-Dialog {
    $script:scope=$root
    $request=@{value=$null}
    Wait-Until {
        $request.value=@((Model).state.requests|Where-Object {$_.kind.request.type -eq 'confirm_close'})
        $request.value.Count -eq 1
    } 'Missing shared unsaved request'
    $title=$request.value[0].kind.request.title
    $script:scope=Control $title ([System.Windows.Automation.ControlType]::Window)
}
function Picker([string]$Name) {
    $script:scope=$root
    $script:scope=Control $Name ([System.Windows.Automation.ControlType]::Window)
    if($scope.Current.ClassName -ne '#32770' -or $scope.Current.ProcessId -ne $review.Id){throw 'Picker does not belong to the isolated review'}
}
function Picker-Button([string]$Id) {
    $hit=@{item=$null}
    Wait-Until {
        $hit.item=$scope.FindFirst([System.Windows.Automation.TreeScope]::Children,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
        $hit.item -and $hit.item.Current.ClassName -eq 'Button' -and $hit.item.Current.IsEnabled
    } 'Native picker button did not become ready'
    $item=$hit.item
    $handle=[IntPtr]$item.Current.NativeWindowHandle
    $owner=[uint32]0;[CapyDocumentControls]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
    if($owner -ne $review.Id){throw 'Native picker button has an unexpected owner'}
    # These standard HWND controls expose no UIA patterns on some Windows builds.
    if(![CapyDocumentControls]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Cannot invoke native picker button'}
}
function Choose-Path([string]$Path) {
    if(!(Split-Path -Parent $Path).Equals($run,[StringComparison]::OrdinalIgnoreCase)){throw 'File fixture must stay in its owned directory'}
    $entry=$null
    Wait-Until {
        $script:pickerEntry=$scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.AndCondition]::new(
                [System.Windows.Automation.OrCondition]::new(
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
        $null -ne $script:pickerEntry
    } 'Missing review picker filename'
    $entry=$script:pickerEntry
    if($entry.Current.ProcessId -ne $review.Id){throw 'Wrong picker filename owner'}
    [CapyDocumentControls]::TypeText([IntPtr]$entry.Current.NativeWindowHandle,$Path)
    Picker-Button '1'
}
function Stroke-InkPixels {
    $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try {
        $rect=[CapyDocumentControls+Rect]::new()
        if(![CapyDocumentControls]::GetClientRect($review.MainWindowHandle,[ref]$rect)){throw 'No canvas bounds'}
        $bitmap=[Drawing.Bitmap]::new($rect.right,$rect.bottom)
        try {
            $graphics=[Drawing.Graphics]::FromImage($bitmap)
            try{
                $origin=[CapyDocumentControls+Point]::new()
                if(![CapyDocumentControls]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'No canvas origin'}
                $graphics.CopyFromScreen($origin.x,$origin.y,0,0,$bitmap.Size)
            }finally{$graphics.Dispose()}
            # Early pen segment, away from its current cursor and chrome.
            $left=[int][Math]::Floor($rect.right*0.4)+20;$top=[int][Math]::Floor($rect.bottom/2)+10
            $ink=0
            for($y=$top;$y -lt $top+20;$y++){for($x=$left;$x -lt $left+30;$x++){
                $pixel=$bitmap.GetPixel($x,$y)
                if($pixel.R -lt 128 -and $pixel.G -lt 128 -and $pixel.B -lt 128){$ink++}
            }}
            $ink
        }finally{$bitmap.Dispose()}
    }finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
}
function Recovery-Events {
    foreach($line in Get-Content (Join-Path $directory 'lifecycle.log') -ErrorAction SilentlyContinue){
        $parts=$line.Split(' ')
        if($parts.Count -eq 3 -and $parts[0] -eq [string]$review.Id){
            [pscustomobject]@{time=[uint64]$parts[1];event=$parts[2]}
        }
    }
}
function Start-GpuLoss([string[]]$Queued=@()) {
    $script:scope=$root
    $before=@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing').Count
    Invoke-Control 'Test GPU loss'
    if(!$Queued.Count){return}
    Wait-Until {@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing').Count -gt $before} 'GPU reconstruction did not start' 30
    $start=@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing')[-1].time
    foreach($action in $Queued){Invoke-Control $action}
    $endEvent=if($FailGpu){'gpu_recovery_save_available'}else{'gpu_recovery_prepared'}
    Wait-Until {@(Recovery-Events|Where-Object {$_.event -eq $endEvent -and $_.time -gt $start}).Count -gt 0} 'GPU reconstruction boundary did not finish' 30
    $end=@(Recovery-Events|Where-Object {$_.event -eq $endEvent -and $_.time -gt $start})[0].time
    $events=@(Recovery-Events|Where-Object {$_.time -gt $start -and $_.time -lt $end -and $_.event.StartsWith('test_pen_')})
    $expected=@($Queued|ForEach-Object {switch($_){'Test pen' {'test_pen_queued'};'Test pen begin' {'test_pen_begin_queued'};'Test pen end' {'test_pen_end_queued'};default {throw 'Unknown pen replay stage'}}})
    if(($events.event -join ',') -ne ($expected -join ',')){throw 'Pen replay missed the reconstruction interval; this run does not prove queued input recovery'}
}
function Fail-Gpu([string[]]$Queued=@()) {
    if(!$FailGpu){return}
    $script:scope=$root
    $revision=(Model).state.document_file.revision
    Start-GpuLoss $Queued
    Wait-Until {(Model).windows_rendering_suspended} 'Exhausted GPU recovery did not preserve save services' 30
    if((Model).state.document_file.revision -ne $revision){throw 'GPU failure changed committed drawing history'}
    foreach($id in @('save_document','save_document_as','close_document')){
        if(!((Model).state.commands|Where-Object id -eq $id).enabled){throw "GPU failure disabled $id"}
    }
    foreach($id in @('new_document','open_document','export_document','undo')){
        if(((Model).state.commands|Where-Object id -eq $id).enabled){throw "GPU failure still enables $id"}
    }
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ("gpu-unavailable-"+$review.Id+".png")) -ClientOnly *> (Join-Path $run ("gpu-unavailable-"+$review.Id+".json"))
}
function Start-RecoveryReview([string]$label){
    $script:stderr=Join-Path $run ($label+'.stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
    Write-Output "GPU save review process $($review.Id), $label"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No GPU save review window' 30
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $script:scope=$root
    # GPU readiness can precede the asynchronous restoration of saved Zen/layout.
    Wait-Until {$model=Model;$model.brush_ready -and $model.windows_workspace.ready} 'GPU save review did not restore its workspace' 60
    if((Model).state.workspace.zen_mode){
        (Find-Id 'zen-button').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {!(Model).state.workspace.zen_mode -and !(Model).chrome_hidden} 'Reopened review could not leave Zen'
    }
    Wait-Until {
        $compact=Find-Id 'application-menus';$full=Find-Id 'application-menu-file'
        ($compact -and !$compact.Current.IsOffscreen) -or ($full -and !$full.Current.IsOffscreen)
    } 'Restored application header did not become visible'
}
function Draw {
    $script:scope=$root
    $ink=@((Model).state.layers|Where-Object {$_.label -eq 'Current ink'})|Select-Object -First 1
    if(!$ink){throw 'Controlled stroke has no raster ink layer'}
    $paint=$ink.id
    if((Model).state.layer_tools.editing_layer.id -ne $paint){
        (Find-Id "layer-$paint-content").GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paint} 'Ink layer did not select'
    }
    if(!(Model).state.layer_tools.controls.alpha_lock){throw 'Controlled stroke target is not editable paint'}
    if(!((Model).state.commands|Where-Object id -eq 'pen').selected){
        $tile=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.resolved_control.command -eq 'pen'}|Select-Object -First 1
        (Find-Id "tile-toolbar-$($tile.id)").GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    Wait-Until {(Model).brush_ready -and ((Model).state.commands|Where-Object id -eq 'pen').selected} 'Pen did not become ready' 45
    if((Model).state.layer_tools.mask_editing -or (Model).state.layer_tools.quick_mask -or (Model).state.layer_tools.editing_layer.mask_selected){throw 'Controlled stroke is editing a mask'}
    $script:drawIndex++
    $channel=(.02+.02*$script:drawIndex).ToString('R',[Globalization.CultureInfo]::InvariantCulture)
    $colorTile=((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'}|Select-Object -First 1
    if(!$colorTile){throw 'Controlled stroke has no color toolbar tile'}
    (Find-Id "tile-toolbar-$($colorTile.id)").GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {Find-Id 'tool-drawer'} 'Controlled ink drawer did not open'
    Wait-Until {$control=Find-Id 'color-foreground';$control -and !$control.Current.IsOffscreen} 'Controlled foreground swatch did not appear'
    (Find-Id 'color-foreground').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground paint did not select'
    Wait-Until {$control=Find-Id 'color-edit';$control -and !$control.Current.IsOffscreen} 'Controlled ink edit control did not appear'
    (Find-Id 'color-edit').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {Find-Id 'precise-color-form-0'} 'Controlled ink editor did not open'
    Combo-Select (Find-Id 'precise-color-form-0') {$_.Current.Name -eq 'Linear RGB'}
    $channels=@('.03',$channel,'.025')
    for($i=0;$i -lt 3;$i++){
        $entry=Find-Id "precise-color-0-$i";$entry.SetFocus()
        $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($channels[$i])
        [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
    }
    (Find-Id 'precise-color-apply').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$definition=(Model).paint_pair.definition;$definition.linear_rgb -and [Math]::Abs($definition.linear_rgb[1]-[double]$channel) -lt .000001 -and $definition.rgba[3] -eq 1} 'Controlled ink color did not publish'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Find-Id 'precise-color-apply')} 'Controlled ink editor did not close'
    (Find-Id "tile-toolbar-$($colorTile.id)").GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find-Id 'tool-drawer')} 'Controlled ink drawer did not close'
    $revision=(Model).state.document_file.revision
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified -and (Model).state.document_file.revision -gt $revision} 'Controlled stroke did not modify the drawing'
}
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No review window' 30
$root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
$script:scope=$root
Wait-Until {(Model).brush_ready} 'Canvas startup did not finish' 45
if(!(Model).windows_isolated_settings){throw 'Review must use an isolated profile'}
File-Command 'new_document';New-Dialog
function Combo-Value($Combo){$selection=$Combo.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection();if($selection.Count){$selection[0].Current.Name}}
function Combo-Select($Combo,[scriptblock]$Pick){
    $Combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $hit=@{item=$null};Wait-Until {$hit.item=@($Combo.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))|Where-Object $Pick)[0];$hit.item} 'Missing drop-down choice'
    $hit.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
$blendChoices=@((Model).document_options.creation.blending.choices)
$perceptual=($blendChoices|Where-Object id -eq 'Perceptual').label;$linear=($blendChoices|Where-Object id -eq 'Linear').label
$blending=Find-Id 'document-blending';$depth=Find-Id 'document-depth'
Wait-Until {(Combo-Value $blending) -eq $perceptual -and $blending.Current.IsEnabled} 'New drawings did not default to Perceptual blending'
$startDepth=Combo-Value $depth
Combo-Select $depth {$_.Current.Name -match 'float'}
Wait-Until {(Combo-Value $blending) -eq $linear -and !$blending.Current.IsEnabled} 'Float did not force Linear blending'
Combo-Select $depth {$_.Current.Name -eq $startDepth}
Wait-Until {(Combo-Value $blending) -eq $perceptual -and $blending.Current.IsEnabled} 'Leaving Float lost the Perceptual blending choice'
Set-Size '2+' '96';Invoke-Control 'Create'
Wait-Until {(Find-Id 'document-error').Current.Name} 'Invalid expression did not show validation'
if((Model).state.document_file.epoch -ne 0){throw 'Invalid size replaced the drawing'}
Set-Size '64*2' '96';Invoke-Control 'Create';Idle
Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 96} 'New drawing did not use shared expressions'

# Exercise the actual native image picker, including a focused numeric draft.
$script:scope=$root
if(!@((Model).layout.groups|Where-Object {$_.active -eq 'layers'}).Count){Invoke-Control 'Layers'}
$paint=(Model).state.layer_tools.editing_layer.id
function Import-Start {
    $script:scope=$root
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find-Id 'layer-import';$hit.item -and $hit.item.Current.IsEnabled} 'Import control stayed disabled'
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Picker 'Open'
}
function Import-Idle {
    Wait-Until {$current=Model;$current -and !$current.state.document_file.busy} 'Image import did not finish' 30
    $script:scope=$root
}
$opacity=Find-Id 'layer-opacity'
$opacity.SetFocus();$opacity.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('55')
Import-Start;Picker-Button '2';Import-Idle
Wait-Until {[Math]::Abs((Model).state.layer_tools.editing_layer.opacity-.55) -lt .000001} 'Import picker lost the focused layer draft'
if(@((Model).state.layers).Count -ne 2){throw 'Cancelling image import inserted a layer'}
Invoke-Control 'Undo'
Wait-Until {!(Model).state.document_file.modified} 'Undo did not restore the clean opacity checkpoint'
$badImage=Join-Path $run 'Invalid image.png'
[IO.File]::WriteAllText($badImage,'synthetic invalid image')
Import-Start;Choose-Path $badImage
Wait-Until {(Model).windows_document.stage -eq 'error'} 'Invalid image did not report a recoverable decoder error' 30
$script:scope=$root;$script:scope=Find-Id 'document-workflow';Invoke-Control 'Close';Import-Idle
if(@((Model).state.layers).Count -ne 2 -or (Model).state.document_file.modified){throw 'Invalid image changed the document'}
Import-Start;Choose-Path $imageSource;Import-Idle
Wait-Until {@((Model).state.layers).Count -eq 3 -and (Model).state.layer_tools.editing_layer.label -eq 'Source image 日本語'} 'Image layer was not selected'
if((Model).error){throw 'Successful import did not clear the previous decoder error'}
$imported=(Model).state.layer_tools.editing_layer.id
Wait-Until {(Find-Id "layer-$imported-thumbnail").Current.ItemStatus -eq 'Ready'} 'Imported image thumbnail did not arrive' 15
Invoke-Control 'Apply transform'
Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Placement did not enter undo history'
Invoke-Control 'Undo'
Wait-Until {@((Model).state.layers).Count -eq 2 -and !(Model).state.document_file.modified} 'Import Undo did not restore the clean document'
Invoke-Control 'Redo'
Wait-Until {@((Model).state.layers).Count -eq 3 -and (Model).state.layer_tools.editing_layer.id -eq $imported} 'Import Redo did not restore the image'

File-Command 'save_document';Picker 'Save As';Picker-Button '2';Idle
if((Model).state.document_file.location){throw 'Cancel acknowledged an unsaved drawing'}
File-Command 'save_document';Picker 'Save As';Choose-Path $first;Idle
Wait-Until {(Test-Path -LiteralPath $first) -and (Model).state.document_file.location.uri -eq $first} 'Save did not write the chosen Unicode path'
$hash=(Get-FileHash -LiteralPath $first).Hash
# This generated source is no longer needed; later Open must use embedded pixels.
Remove-Item -LiteralPath $imageSource
Draw
$exported=Join-Path $run 'Export 日本語.png'
if(((Model).state.commands|Where-Object id -eq 'export_again').enabled){throw 'Export Again was available before the first export'}
File-Command 'export_document';Picker 'Save As';Picker-Button '2';Idle
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){
    throw 'Cancelled export changed the project checkpoint'
}
File-Command 'export_document';Picker 'Save As';Choose-Path $exported;Idle
Wait-Until {Test-Path -LiteralPath $exported} 'Export did not create the chosen PNG'
$png=[IO.File]::ReadAllBytes($exported)
if($png.Length -lt 45 -or [Convert]::ToHexString($png[0..7]) -ne '89504E470D0A1A0A' -or
    [Convert]::ToHexString($png[12..23]) -ne '494844520000008000000060' -or
    [Convert]::ToHexString($png[($png.Length-8)..($png.Length-1)]) -ne '49454E44AE426082'){
    throw 'Export must be a complete PNG of the 128 by 96 document'
}
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){
    throw 'PNG export incorrectly acknowledged a project save'
}
if((Model).state.document_file.export_uri -ne $exported){throw 'A finished export did not become the Export Again destination'}
function Export-Again {
    $script:scope=$root;$written=(Get-Item -LiteralPath $exported).LastWriteTimeUtc
    File-Command 'export_again'
    Wait-Until {(Get-Item -LiteralPath $exported).LastWriteTimeUtc -gt $written} 'Export Again did not rewrite its destination' 45
    Idle
    if(Find-Name 'Save As' ([System.Windows.Automation.ControlType]::Window)){throw 'Export Again asked for a destination it can still write'}
}
$hash=(Get-FileHash -LiteralPath $exported).Hash
Export-Again
if((Get-FileHash -LiteralPath $exported).Hash -ne $hash){throw 'Export Again did not rewrite identical pixels'}
Draw
Export-Again
if((Get-FileHash -LiteralPath $exported).Hash -eq $hash){throw 'Export Again did not export the current pixels'}
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){throw 'Export Again acknowledged a project save'}
Remove-Item -LiteralPath $exported
$script:scope=$root;File-Command 'export_again';Picker 'Save As';$moved=Join-Path $run 'Export again.png';Choose-Path $moved;Idle
Wait-Until {(Test-Path -LiteralPath $moved) -and (Model).state.document_file.export_uri -eq $moved} 'Export Again did not fall back to choosing a new destination'
$webp=Join-Path $run 'Export.webp'
File-Command 'export_document' {
    Combo-Select (Find-Id 'export-format') {$_.Current.Name -like 'WebP*'}
    Wait-Until {$format=Find-Id 'export-format';$format -and (Combo-Value $format) -like 'WebP*'} 'Export did not choose WebP'
}
Picker 'Save As';Choose-Path $webp;Idle
Wait-Until {Test-Path -LiteralPath $webp} 'Export did not create the chosen WebP'
$bytes=[IO.File]::ReadAllBytes($webp);$text=[Text.Encoding]::ASCII.GetString($bytes)
if($bytes.Length -lt 30 -or $text.Substring(0,4) -ne 'RIFF' -or $text.Substring(8,8) -ne 'WEBPVP8X' -or !$text.Contains('VP8L') -or
    ($bytes[24]+256*$bytes[25]+65536*$bytes[26]+1) -ne 128 -or ($bytes[27]+256*$bytes[28]+65536*$bytes[29]+1) -ne 96){throw 'Export must be a lossless WebP of the 128 by 96 document'}
$photo=Join-Path $run 'Metadata photo.jpg';$photoExport=Join-Path $run 'Metadata export.jpg'
$bitmap=[Drawing.Bitmap]::new(64,48)
try{
    $graphics=[Drawing.Graphics]::FromImage($bitmap);$graphics.Clear([Drawing.Color]::FromArgb(255,180,120,60));$graphics.Dispose()
    foreach($tag in @(@(0x013B,2,[Text.Encoding]::ASCII.GetBytes("Native Artist`0")),@(0x8298,2,[Text.Encoding]::ASCII.GetBytes("Native Copyright`0")),
        @(0x0001,2,[Text.Encoding]::ASCII.GetBytes("N`0")),@(0x0002,5,[byte[]](51,0,0,0,1,0,0,0,30,0,0,0,1,0,0,0,0,0,0,0,1,0,0,0)))){
        $item=[Runtime.Serialization.FormatterServices]::GetUninitializedObject([Drawing.Imaging.PropertyItem])
        $item.Id=$tag[0];$item.Type=$tag[1];$item.Value=[byte[]]$tag[2];$item.Len=$item.Value.Length;$bitmap.SetPropertyItem($item)
    }
    $bitmap.Save($photo,[Drawing.Imaging.ImageFormat]::Jpeg)
}finally{$bitmap.Dispose()}
function Photo-Tags([string]$Path){$image=[Drawing.Image]::FromFile($Path);try{@($image.PropertyIdList)}finally{$image.Dispose()}}
if(@(Compare-Object (Photo-Tags $photo) @(0x013B,0x8298,0x0001,0x0002) -ExcludeDifferent -IncludeEqual).Count -ne 4){throw 'The photo fixture did not keep its Exif and GPS tags'}
$documentEpoch=(Model).state.document_file.epoch
File-Command 'open_document';Picker 'Open';Choose-Path $photo;Idle
Wait-Until {(Model).state.document_file.epoch -gt $documentEpoch} 'Open did not select the photo'
File-Command 'export_document' {
    $format=Find-Id 'export-format';Combo-Select $format {$_.Current.Name -eq 'JPEG image'}
    Wait-Until {$format=Find-Id 'export-format';$format -and (Combo-Value $format) -eq 'JPEG image'} 'Export did not choose JPEG'
    Wait-Until {$script:metadata=Find-Id 'export-metadata';$script:metadata -and !$script:metadata.Current.IsOffscreen} 'Export did not offer Metadata for a photo'
    $remove=Find-Id 'export-remove-location'
    if(!$remove){throw "Keeping $(Combo-Value $script:metadata) metadata did not offer Remove location"}
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$scroller=$walker.GetParent($remove);$scroll=$null
    while($scroller -and !$scroller.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$scroller=$walker.GetParent($scroller)}
    if(!$scroll){throw 'The export options do not scroll'}
    $scroll.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,100)
    Wait-Until {!(Find-Id 'export-remove-location').Current.IsOffscreen} 'Remove location could not be scrolled into view'
    Combo-Select $script:metadata {$_.Current.Name -eq 'Copyright & Contact'}
    Wait-Until {!(Find-Id 'export-remove-location')} 'Remove location stayed for Copyright & Contact'
}
Picker 'Save As';Choose-Path $photoExport;Idle
Wait-Until {Test-Path -LiteralPath $photoExport} 'Photo export did not create the chosen JPEG'
$kept=Photo-Tags $photoExport
if(0x013B -notin $kept -or 0x8298 -notin $kept){throw 'Copyright & Contact dropped the artist or copyright'}
if(0x0001 -in $kept -or 0x0002 -in $kept){throw 'Copyright & Contact kept the location'}
$script:scope=$root
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Discard Changes';$script:scope=$root;Idle
Wait-Until {(Model).state.document_file.location.uri -eq $first} 'Closing the photo did not return to the drawing'

if($RecoverGpu){
    function Signature {
        $state=(Model).state
        $state.document_file.PSObject.Properties.Remove("revision")
        @($state.document_file,$state.camera,$state.workspace,$state.brush)|ConvertTo-Json -Depth 80 -Compress
    }
    $beforeRecovery=Signature
    $exportHash=(Get-FileHash -LiteralPath $exported -Algorithm SHA256).Hash
    foreach($attempt in 1..2){
        $generation=(Model).windows_gpu_generation
        $documentRevision=(Model).state.document_file.revision
        $script:scope=$root
        Invoke-Control 'Undo'
        Wait-Until {(Model).state.document_file.revision -gt $documentRevision} 'Pre-recovery Undo did not finish'
        $undone=(Model).state.document_file.revision
        if($attempt -eq 2){
            Wait-Until {(Stroke-InkPixels) -eq 0} 'Pre-recovery Undo did not reach the canvas'
            Invoke-Control 'Test pen begin'
            Wait-Until {(Stroke-InkPixels) -ge 100} 'Active pen stroke did not render before removal'
            & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'active-pen-before-loss.png') -ClientOnly *> (Join-Path $run 'active-pen-before-loss.json')
            Start-GpuLoss @('Test pen end')
        }else{
            Start-GpuLoss @('Test pen')
        }
        Wait-Until {(Model).windows_gpu_generation -eq $generation+1 -and (Model).brush_ready} 'GPU reconstruction did not finish' 60
        Wait-Until {(Model).state.document_file.revision -gt $undone} 'GPU recovery lost queued pen samples'
        $documentRevision=(Model).state.document_file.revision
        Wait-Until {(Signature) -eq $beforeRecovery} 'GPU recovery changed document, history, camera, workspace or brush'
        $status=Find-Id 'canvas-status'
        if($status -and !$status.Current.IsOffscreen){throw ('GPU recovery reports an error: '+$status.Current.Name)}
        $restored=Join-Path $run ("Recovered-$attempt.png")
        File-Command 'export_document';Picker 'Save As';Choose-Path $restored;Idle
        Wait-Until {Test-Path -LiteralPath $restored} 'Recovered GPU did not export'
        if((Get-FileHash -LiteralPath $restored -Algorithm SHA256).Hash -ne $exportHash){
            throw 'GPU reconstruction changed exported pixels'
        }
        $script:scope=$root
        Invoke-Control 'Undo'
        Wait-Until {(Model).state.document_file.revision -ne $documentRevision} 'Undo after reconstruction did not change the document'
        $undoRevision=(Model).state.document_file.revision
        Invoke-Control 'Redo'
        Wait-Until {(Model).state.document_file.revision -ne $undoRevision -and (Signature) -eq $beforeRecovery} 'Redo after reconstruction did not restore the drawing'
        Wait-Until {$thumbnail=Find-Id "layer-$imported-thumbnail";$thumbnail -and $thumbnail.Current.ItemStatus -eq 'Ready'} 'Imported image thumbnail did not recover' 15
    }
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'gpu-recovered.png') -ClientOnly *> (Join-Path $run 'gpu-capture.json')
}

File-Command 'save_document';Idle
Wait-Until {$current=Model;$current -and !$current.state.document_file.modified} 'Save to existing location did not clear the captured checkpoint'
if((Get-FileHash -LiteralPath $first).Hash -eq $hash){throw 'Save did not update the source project'}
File-Command 'save_document_as';Picker 'Save As';Picker-Button '2';Idle
if((Model).state.document_file.location.uri -ne $first){throw 'Cancelled Save As changed location'}
File-Command 'save_document_as';Picker 'Save As';Choose-Path $second;Idle
Wait-Until {(Test-Path -LiteralPath $second) -and (Model).state.document_file.location.uri -eq $second} 'Save As did not adopt the new path'
$epoch=(Model).state.document_file.epoch
File-Command 'open_document';Picker 'Open';Choose-Path $corrupt;Idle
Wait-Until {(Model).state.host_error -or (Model).error} 'Corrupt Open did not report failure'
if((Model).state.document_file.epoch -ne $epoch -or (Model).state.document_file.location.uri -ne $second){throw 'Corrupt Open replaced the live drawing'}
Draw
$drawingCount=@((Model).windows_tabs.tabs).Count
File-Command 'new_document';New-Dialog;Invoke-Control 'Cancel';Idle
if((Model).state.document_file.epoch -ne $epoch -or !(Model).state.document_file.modified){throw 'Cancelled New lost edits'}
File-Command 'open_document';Picker 'Open';Picker-Button '2';Idle
if((Model).state.document_file.epoch -ne $epoch -or !(Model).state.document_file.modified){throw 'Cancelled Open lost edits'}
File-Command 'save_document';Idle
Wait-Until {!(Model).state.document_file.modified} 'Explicit Save did not complete'
File-Command 'open_document';Picker 'Open';Choose-Path $first;Idle
Wait-Until {(Model).state.document_file.epoch -gt $epoch -and (Model).state.document_file.location.uri -eq $first} 'Open did not select the new drawing'
if(@((Model).windows_tabs.tabs).Count -ne $drawingCount+1){throw 'Open did not retain the existing drawings'}
if((Model).state.document_file.modified){throw 'Opened project is unexpectedly dirty'}
Draw
Fail-Gpu
if($FailGpu){File-Command 'save_document_as';Picker 'Save As';Picker-Button '2';Idle}
$script:scope=$root
& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
(Control 'Preferences' ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
$script:scope=Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
$custom=@{item=$null};Wait-Until {
    $custom.item=@($scope.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like 'setting-dark_base-swatch-*'})[0]
    $null -ne $custom.item
} 'Missing custom dark base swatch'
$custom.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
$entry=Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)
$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#223344')
Request-Close -WithPreferences
Wait-Closed 'Session close did not finish'
if((Get-Item -LiteralPath $stderr).Length){throw 'Native review reported stderr'}
$stderr=Join-Path $run 'reopened.stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No restarted review window' 30
$root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle);$script:scope=$root
Wait-Until {(Model).brush_ready -and !(Model).windows_recovery.busy -and !(Model).windows_recovery.restoring} 'Session did not reopen' 120
if((Model).state.settings.dark_base -ne '#223344' -or !(Model).state.document_file.modified){throw 'Window close lost the focused Preferences draft or unsaved drawing'}
Request-Close;Wait-Closed 'Restarted session close did not finish'
if((Get-Item -LiteralPath $stderr).Length){throw 'Restarted native review reported stderr'}
$env:CAPY_STORAGE_DIR=Join-Path $run 'untitled-profile'
$stderr=Join-Path $run 'untitled.stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Untitled close review process $($review.Id)"
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'No untitled review window' 30
$root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
$script:scope=$root
Wait-Until {(Model).brush_ready} 'Untitled canvas did not finish starting' 45
Draw
Fail-Gpu
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Save';Picker 'Save As';Picker-Button '2';Idle
if(!(Model).state.document_file.modified -or (Model).state.document_file.location -or (Model).state.document_file.close_ready){
    throw 'Cancelling the close-time picker lost the untitled drawing'
}
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Cancel';Idle
File-Command 'close_document';Confirm-Dialog;Invoke-Control 'Discard Changes'
Wait-Closed 'Discarded close did not finish'
if((Get-Item -LiteralPath $stderr).Length){throw 'Untitled native review reported stderr'}
if($FailGpu){
    Start-RecoveryReview 'save-as'
    Draw
    $before=Join-Path $run 'Before GPU failure.png'
    File-Command 'export_document';Picker 'Save As';Choose-Path $before;Idle
    Wait-Until {Test-Path -LiteralPath $before} 'Baseline export did not finish'
    $script:scope=$root
    $revision=(Model).state.document_file.revision
    (Find-Id 'zen-button').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Model).state.workspace.zen_mode -and (Model).chrome_hidden} 'Zen did not hide the editor before GPU failure'
    # Completed raster pixels remain saveable. Contacts admitted during failed
    # reconstruction must be canceled without preventing the save workflow.
    Fail-Gpu @('Test pen','Test pen begin')
    if((Model).state.document_file.revision -ne $revision){throw 'Failed reconstruction changed the completed raster boundary'}
    if(!(Model).state.workspace.zen_mode){throw 'GPU failure rewrote the saved Zen preference'}
    $recovered=Join-Path $run 'Saved after GPU failure.capy'
    File-Command 'save_document_as';Picker 'Save As';Choose-Path $recovered;Idle
    Wait-Until {(Test-Path -LiteralPath $recovered) -and !(Model).state.document_file.modified} 'Save As after GPU failure did not complete durably'
    File-Command 'save_document';Idle
    Request-Close
    Wait-Closed 'GPU save-as close did not finish'
    if((Get-Item -LiteralPath $stderr).Length){throw 'GPU save-as reported stderr'}
    Remove-Item Env:CAPY_TEST_GPU_UNAVAILABLE
    Start-RecoveryReview 'reopen'
    File-Command 'open_document';Picker 'Open';Choose-Path $recovered;Idle
    Wait-Until {(Model).state.document_file.location.uri -eq $recovered} 'Saved recovery project did not reopen'
    $after=Join-Path $run 'After GPU failure.png'
    File-Command 'export_document';Picker 'Save As';Choose-Path $after;Idle
    Wait-Until {Test-Path -LiteralPath $after} 'Reopened project did not export'
    if((Get-FileHash -LiteralPath $before).Hash -ne (Get-FileHash -LiteralPath $after).Hash){throw 'Saved recovery project changed exported pixels'}
    Request-Close
    Wait-Closed 'Reopened recovery project close did not finish'
    if((Get-Item -LiteralPath $stderr).Length){throw 'Reopened recovery project reported stderr'}
}
[PSCustomObject]@{
    gpu_failure_save=if($FailGpu){'Save/Save As, committed raster preservation and queued contact cancellation, Cancel, Discard, durable reopen and identical exported pixels passed'}else{'not requested'}
    shared_new_size_and_validation='passed'
    new_document_blending_survives_float='passed'
    image_picker_draft_cancel_and_error_recovery='passed'
    image_layer_thumbnail_undo_redo_and_embedded_reopen='passed'
    save_cancel_and_unicode_path='passed'
    png_export_cancel_dimensions_and_checkpoint='passed'
    lossless_webp_export='passed'
    photo_metadata_copyright_without_location='passed'
    gpu_recovery=if($RecoverGpu){'two replacements, queued and active pen strokes, identical exported pixels, thumbnails and Undo/Redo passed'}else{'not requested'}
    save_existing_and_save_as='passed'
    corrupt_open_preserves_live_document='passed'
    retained_drawings_picker_cancel_and_explicit_save='passed'
    preferences_draft_and_cancelled_close='passed'
    durable_save_then_close='passed'
    untitled_close_picker_cancel_and_discard='passed'
    scope='isolated native controls and pickers; controlled stroke replay, not physical pen or cadence acceptance'
}|ConvertTo-Json
} finally {
    Exit-CapyEnvironment
}
