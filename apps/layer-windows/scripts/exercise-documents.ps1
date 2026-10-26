param([Parameter(Mandatory)][string]$Executable,[string]$DebuggerPath,[switch]$RecoverGpu,[switch]$FailGpu,[ValidateSet('full','new-drawing','color-workflows','color-workflows-light','native-writeback','native-writeback-light')][string]$Journey='full')
if($RecoverGpu -and $FailGpu){throw "Choose successful recovery or exhausted recovery, not both"}
if($Journey -ne 'full' -and ($RecoverGpu -or $FailGpu)){throw 'GPU recovery requires the full document journey'}
$ErrorActionPreference='Stop'
$focusedColor=$Journey.StartsWith('color-workflows')
$nativeWriteback=$Journey.StartsWith('native-writeback')
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'PackageFixture.cs') -ReferencedAssemblies @(
    'System.IO.Compression.dll','System.IO.Compression.FileSystem.dll'
    if($PSVersionTable.PSEdition -eq 'Core'){'System.Collections.dll';'System.IO.Compression.ZipFile.dll'}
)
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
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect rect);
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
if($focusedColor -or $nativeWriteback){[IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$(if($Journey.EndsWith('-light')){'light'}else{'dark'})}|ConvertTo-Json -Depth 4))}
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
$null=$review.Handle
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Document review process $($review.Id)"
function Bind-ReviewWindow([string]$Message) {
    Wait-Until {$script:drawing=Owned-DrawingWindow $review;$null-ne$script:drawing} $Message 30
    $script:root=$drawing.Root;$script:scope=$root
    if($DebuggerPath){$script:debugger=Start-CapyDebugger $review $DebuggerPath $run $drawing}
}
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
    $handle=$drawing.Handle
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
    $script:drawing=$null;$script:root=$null;$script:scope=$null
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
function Choose-Item($Item) {
    $pattern=$null
    if($Item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    elseif($Item.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)){$pattern.Select()}
    else{$Item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Named-Control([string]$Name,$Type=[System.Windows.Automation.ControlType]::Button) {
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
function Drawing {(Model).state.new_drawing}
function Drawer {$view=Drawing;if($view){$view.drawer}}
function Owned([string]$Id) {$hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Within $root;$null -ne $hit.item} "Missing control: $Id";$hit.item}
function Press([string]$Id) {
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Id -Within $root;$hit.item -and $hit.item.Current.IsEnabled} "Missing enabled control: $Id"
    $hit.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Foreground {[CapyRowPointer]::SetForegroundWindow($drawing.Handle)|Out-Null;Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $drawing.Handle} 'The review did not take foreground input'}
function Type-Text([string]$Id,[string]$Text,[switch]$Commit) {
    $entry=Owned $Id;$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
    if($Commit){Foreground;[CapyRowPointer]::Key([uint32]$review.Id,0x0D)}
}
function Pick-Choice([string]$Id,[int]$Index) {
    $box=Owned $Id;$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $hit=@{item=$null};Wait-Until {$hit.item=Find "$Id-$Index" -Within $box;$null -ne $hit.item} "Missing choice $Index in $Id"
    $hit.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Item-Index($List,[scriptblock]$Match) {$items=@($List);for($i=0;$i -lt $items.Count;$i++){if(& $Match $items[$i]){return $i}};throw 'The New drawing view has no matching item'}
function Value-Index($List,$Value) {Item-Index $List {param($item)$item.value -eq $Value}}
function Field-Value([string]$Id) {[double](((Owned $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value) -replace '[^0-9.]','')}
function Import-Last {
    $last=@((Drawer).tiles).Count-1
    Scroll-Position (Owned 'new-drawing-paper-import') 100
    Wait-Until {
        $tile=(Owned "new-drawing-paper-tile-$last").Current.BoundingRectangle;$import=(Owned 'new-drawing-paper-import').Current
        $after=if([Math]::Abs($import.BoundingRectangle.Top-$tile.Top) -le 1){$import.BoundingRectangle.Left -ge $tile.Right}else{$import.BoundingRectangle.Top -ge $tile.Bottom}
        $after -and $import.Name -eq (Drawing).copy.import -and [Math]::Abs($import.BoundingRectangle.Width-$tile.Width) -le 1 -and [Math]::Abs($import.BoundingRectangle.Height-$tile.Height) -le 1
    } 'The Import tile is not a paper-sized tile named Import after the last paper'
}
function Click-Center([string]$Id) {
    $bounds=(Owned $Id).Current.BoundingRectangle
    $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try{
        [CapyRowPointer]::SetForegroundWindow($drawing.Handle)|Out-Null
        [CapyRowPointer]::Initialize([uint32]$review.Id)
        try{[CapyRowPointer]::Down('mouse',[int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2));[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Dispose()}
    }finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
}
function Resize-Review([int]$Width,[int]$Height) {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Resize -Width $Width -Height $Height
}
function Context-Item([string]$Target,[string]$Item) {
    (Owned $Target).SetFocus();Wait-Until {(Find $Target -Within $root).Current.HasKeyboardFocus} "Context target did not take focus: $Target"
    Foreground;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
    Press $Item
}
function New-Capture([string]$Name) {Start-Sleep -Milliseconds 800;Capture "new-drawing-$Name-$($script:newTheme)" -WithModel}
function Open-NewDrawing {
    File-Command 'new_document'
    Wait-Until {(Drawing) -and (Find 'new-drawing-dialog' -Within $root) -and (Find 'document-width' -Within $root)} 'New drawing did not open' 45
    if((Drawing).untitled -ne (Model).windows_tabs.upcoming_untitled){throw 'New drawing did not offer the next untitled name'}
}
function Close-NewDrawing([string]$Id) {
    Press $Id
    Wait-Until {!(Drawing) -and !(Find 'new-drawing-dialog' -Within $root) -and !@((Model).state.requests|Where-Object {$_.kind.type -eq 'document'}).Count} 'New drawing did not close'
}
function Open-Drawer([string]$Id,[string]$Kind) {
    Press $Id;Wait-Until {(Drawer).kind -eq $Kind -and !(Find $(switch($Kind){'presets'{'new-drawing-browser'}'paper'{'new-drawing-paper-drawer'}default{'new-drawing-color-drawer'}}) -Within $root).Current.IsOffscreen} "The $Kind drawer did not open"
}
function New-Journey([string]$Theme,[switch]$Full) {
    $script:newTheme=$Theme;$script:scope=$root
    Open-NewDrawing
    New-Capture 'main'
    Press 'new-drawing-resolution-choices'
    Wait-Until {Find 'new-drawing-resolution-0' -Within $root} 'Common resolutions did not open'
    New-Capture 'resolutions'
    $start=(Drawing).setup.resolution
    Press "new-drawing-resolution-$(if($Full){0}else{Value-Index (Drawing).resolutions $start})"
    Wait-Until {!(Find 'new-drawing-resolution-0' -Within $root) -and (Drawing).setup.resolution -eq $(if($Full){72}else{$start})} 'A common resolution did not apply'
    Open-Drawer 'new-drawing-presets-button' 'presets'
    New-Capture 'presets'
    if($Full){
        Type-Text 'new-drawing-search' 'chalk'
        Wait-Until {$browser=Drawer;$browser.query -eq 'chalk' -and !$browser.tabs_enabled -and @($browser.tiles).Count -ge 1 -and $browser.tiles[0].name -eq 'Chalkboard'} 'Preset search did not find Chalkboard'
        Type-Text 'new-drawing-search' ''
        Wait-Until {$browser=Drawer;$browser.query -eq '' -and $browser.tabs_enabled} 'Clearing the search did not restore the tabs'
        $tab=Item-Index (Drawer).tabs {param($item)$item.label -eq 'Pastel'}
        Press "new-drawing-tab-$tab"
        Wait-Until {(Drawer).tabs[$tab].selected} 'The Pastel tab did not open'
        Press 'new-drawing-preset-1'
        Wait-Until {$browser=Drawer;$browser.tiles[1].selected -and $browser.summary.name -eq $browser.tiles[1].name} 'Picking a preset did not update the summary'
    }
    Press 'new-drawing-save-as'
    Wait-Until {(Drawer).save} 'Save As did not open the naming row'
    New-Capture 'save'
    if($Full){
        Type-Text 'new-drawing-save-name' 'Studio wall'
        Wait-Until {(Drawer).save.name -eq 'Studio wall' -and (Drawer).save.can_save} 'The preset name did not reach the draft'
        Press 'new-drawing-save-confirm'
        Wait-Until {$browser=Drawer;!$browser.save -and @((Model).state.settings.new_document.setups|Where-Object name -eq 'Studio wall').Count -eq 1 -and @($browser.tiles|Where-Object {$_.own -and $_.selected}).Count -eq 1} 'Save As did not store and select the preset'
        $own=Item-Index (Drawer).tiles {param($item)$item.name -eq 'Studio wall'}
        Context-Item "new-drawing-preset-$own" "new-drawing-preset-rename-$own"
        Wait-Until {(Drawer).save.name -eq 'Studio wall'} 'Rename did not open the naming row'
        Type-Text 'new-drawing-save-name' 'Studio wall 2'
        Wait-Until {(Drawer).save.name -eq 'Studio wall 2'} 'The new preset name did not reach the draft'
        Press 'new-drawing-save-confirm'
        Wait-Until {!(Drawer).save -and @((Model).state.settings.new_document.setups.name) -contains 'Studio wall 2' -and @((Model).state.settings.new_document.setups.name) -notcontains 'Studio wall'} 'Rename did not rename the preset'
        $own=Item-Index (Drawer).tiles {param($item)$item.name -eq 'Studio wall 2'}
        Context-Item "new-drawing-preset-$own" "new-drawing-preset-delete-$own"
        Wait-Until {!(Model).state.settings.new_document.setups} 'Delete did not remove the preset'
        Press 'new-drawing-preset-0'
        Wait-Until {(Drawer).tiles[0].selected -and (Drawer).can_select} 'The first preset did not select'
        $chosen=(Drawer).tiles[0].name
        Press 'new-drawing-select'
        Wait-Until {!(Drawer) -and (Drawing).page.preset -eq $chosen} 'Select did not use the preset'
        Open-Drawer 'new-drawing-presets-button' 'presets'
        Foreground;[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
        Wait-Until {!(Drawer) -and (Drawing)} 'Escape did not close only the presets drawer'
    }else{
        Press 'new-drawing-save-cancel';Wait-Until {!(Drawer).save} 'Cancel did not leave the naming row'
        Press 'new-drawing-browser-cancel';Wait-Until {!(Drawer)} 'Cancel did not close the presets'
    }
    Open-Drawer 'new-drawing-presets-button' 'presets'
    Type-Text 'new-drawing-search' 'chalk'
    Wait-Until {$browser=Drawer;$browser.query -eq 'chalk' -and @($browser.tiles).Count -ge 1 -and $browser.tiles[0].name -eq 'Chalkboard'} 'Preset search did not find Chalkboard'
    Press 'new-drawing-preset-0';Wait-Until {(Drawer).tiles[0].selected} 'Chalkboard did not select'
    Press 'new-drawing-select'
    Wait-Until {$view=Drawing;!$view.drawer -and $view.page.preset -eq 'Chalkboard' -and $view.setup.paper.relief -eq 50 -and $view.page.relief -eq '50 %'} 'Chalkboard did not set its texture visibility'
    Wait-Until {(Owned 'new-drawing-texture-relief').Current.Name -eq '50 %'} 'The texture tile did not show the texture visibility'
    $tile=(Owned 'new-drawing-texture').Current.BoundingRectangle;$color=(Owned 'new-drawing-paper-color').Current.BoundingRectangle
    if([Math]::Abs($tile.Height-$color.Height) -gt 1){throw "The texture visibility line changed the tile height: $($tile.Height) and $($color.Height)"}
    New-Capture 'main-chalkboard'
    Open-Drawer 'new-drawing-texture' 'paper'
    Wait-Until {$drawer=Drawer;$drawer.relief.value -eq 50 -and $drawer.relief.enabled -and (Owned 'new-drawing-paper-relief').Current.IsEnabled -and (Field-Value 'new-drawing-paper-relief') -eq 50} 'Texture visibility did not show the preset value'
    New-Capture 'paper'
    $marked=@((Drawer).tiles|Where-Object selected)[0].id
    Import-Last
    New-Capture 'paper-import'
    foreach($activate in @({Click-Center 'new-drawing-paper-import'},{(Owned 'new-drawing-paper-import').SetFocus();Foreground;[CapyRowPointer]::Key([uint32]$review.Id,0x20)})){
        & $activate;Picker 'Open';Picker-Button '2';$script:scope=$root
        Wait-Until {!(Find-Name 'Open' ([System.Windows.Automation.ControlType]::Window))} 'The paper picker did not close'
        Wait-Until {$drawer=Drawer;$drawer.kind -eq 'paper' -and @($drawer.tiles|Where-Object selected).id -eq $marked -and !(Owned 'new-drawing-paper-import').Current.ItemStatus} 'Import changed the marked paper'
    }
    Press 'new-drawing-paper-tile-0'
    Wait-Until {$drawer=Drawer;$drawer.tiles[0].selected -and !$drawer.relief.enabled -and !(Owned 'new-drawing-paper-relief').Current.IsEnabled} 'No paper did not disable Texture visibility'
    New-Capture 'paper-none'
    Press 'new-drawing-paper-use'
    Wait-Until {$view=Drawing;!$view.drawer -and !$view.setup.paper.texture -and $null -eq $view.page.relief -and !(Find 'new-drawing-texture-relief' -Within $root)} 'No paper kept the texture visibility line'
    Open-Drawer 'new-drawing-texture' 'paper'
    $paper=Item-Index (Drawer).tiles {param($item)$item.id -eq 'cold_press'}
    Press "new-drawing-paper-tile-$paper"
    Wait-Until {$drawer=Drawer;$drawer.tiles[$paper].selected -and $drawer.relief.enabled -and (Owned 'new-drawing-paper-relief').Current.IsEnabled} 'The paper tile did not select'
    Type-Text 'new-drawing-paper-relief' '150'
    Press 'new-drawing-paper-use'
    Wait-Until {$view=Drawing;!$view.drawer -and $view.setup.paper.texture -eq 'cold_press' -and $view.setup.paper.relief -eq 150 -and $view.page.relief -eq '150 %'} 'Use Texture did not apply the typed texture visibility'
    Wait-Until {(Owned 'new-drawing-texture-relief').Current.Name -eq '150 %'} 'The texture tile did not show the new texture visibility'
    Open-Drawer 'new-drawing-texture' 'paper'
    Wait-Until {(Field-Value 'new-drawing-paper-relief') -eq 150} 'Texture visibility did not reopen with the sheet value'
    Type-Text 'new-drawing-paper-relief' '20'
    Press 'new-drawing-paper-cancel'
    Wait-Until {$view=Drawing;!$view.drawer -and $view.setup.paper.relief -eq 150} 'Cancel kept the typed texture visibility'
    Open-Drawer 'new-drawing-texture' 'paper'
    Wait-Until {(Field-Value 'new-drawing-paper-relief') -eq 150} 'The cancelled draft stayed in the Texture visibility field'
    if($Full){
        $footer={$parts=@{};foreach($part in @('relief-label','relief','cancel','use')){$parts[$part]=(Owned "new-drawing-paper-$part").Current.BoundingRectangle};$parts}
        $wide=& $footer
        $frame=[CapyDocumentControls+Rect]::new();$context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
        try{if(![CapyDocumentControls]::GetWindowRect($drawing.Handle,[ref]$frame)){throw 'Cannot read the review window bounds'}}finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
        Resize-Review ([int](420*[CapyRowPointer]::GetDpiForWindow($drawing.Handle)/96.)) ($frame.bottom-$frame.top)
        $narrow=@{parts=$null}
        Wait-Until {$narrow.parts=& $footer;$narrow.parts['cancel'].Left -lt $wide['cancel'].Left-100} 'The paper footer did not narrow'
        Wait-Until {
            $parts=$narrow.parts=& $footer
            $parts['relief-label'].Width -gt 0 -and $parts['relief'].Left -gt $parts['relief-label'].Left -and $parts['relief'].Right -le $parts['cancel'].Left -and $parts['cancel'].Right -le $parts['use'].Left
        } 'Texture visibility overflowed the narrow paper footer'
        foreach($size in @(@{name='wide';parts=$wide},@{name='narrow';parts=$narrow.parts})){"Paper footer $($size.name): $(($size.parts.GetEnumerator()|Sort-Object Key|ForEach-Object {"$($_.Key)=$($_.Value)"}) -join '; ')"}
        New-Capture 'paper-narrow'
        Resize-Review ($frame.right-$frame.left) ($frame.bottom-$frame.top)
        Wait-Until {[Math]::Abs((& $footer)['cancel'].Left-$wide['cancel'].Left) -le 2} 'The review did not return to its width'
    }
    Press 'new-drawing-paper-cancel';Wait-Until {!(Drawer)} 'Cancel did not close the paper drawer'
    if($Full){
        $paperFile=Join-Path $run 'Fixture paper.png'
        $grain=[Drawing.Bitmap]::new(128,128);$random=[Random]::new(7)
        try{for($y=0;$y -lt 128;$y++){for($x=0;$x -lt 128;$x++){$v=150+$random.Next(80);$grain.SetPixel($x,$y,[Drawing.Color]::FromArgb(255,$v,$v,$v))}};$grain.Save($paperFile,[Drawing.Imaging.ImageFormat]::Png)}finally{$grain.Dispose()}
        Open-Drawer 'new-drawing-texture' 'paper'
        Press 'new-drawing-paper-import';Picker 'Open';Choose-Path $paperFile;$script:scope=$root
        Wait-Until {@((Drawer).tiles|Where-Object {$_.selected -and $_.label -eq 'Fixture paper'}).Count -eq 1} 'Import did not pick the imported paper' 45
        Import-Last
        $imported=@((Drawer).tiles|Where-Object selected)[0].id
        Press 'new-drawing-paper-use'
        Wait-Until {!(Drawer) -and (Drawing).setup.paper.texture -eq $imported -and (Drawing).page.texture -eq 'Fixture paper'} 'The imported paper was not used'
    }
    Open-Drawer 'new-drawing-paper-color' 'paper_color'
    New-Capture 'paper-color'
    if($Full){
        Press 'new-drawing-paper-color-5'
        Wait-Until {$colors=Drawer;$colors.palette[5].selected -and !$colors.transparent -and !$colors.can_add} 'The palette color did not reach the editor'
        Press 'edit-color-hex';Type-Text 'edit-color-hex-entry' '#E07090' -Commit
        Wait-Until {(Drawer).can_add} 'A new color did not offer Add'
        $count=@((Drawer).palette).Count
        Press 'new-drawing-add-paper-color'
        Wait-Until {@((Drawer).palette).Count -eq $count+1} 'Add did not store the paper color'
        Context-Item "new-drawing-paper-color-$count" 'new-drawing-paper-color-delete'
        Wait-Until {@((Drawer).palette).Count -eq $count} 'Delete did not remove the paper color'
    }
    Press 'new-drawing-color-mode-transparent'
    Wait-Until {(Drawer).transparent} 'Transparent did not apply'
    New-Capture 'transparent'
    if(!$Full){Press 'new-drawing-color-cancel';Wait-Until {!(Drawer)} 'Cancel did not close the paper color';Close-NewDrawing 'new-drawing-cancel';return}
    Press 'new-drawing-color-use'
    Wait-Until {$view=Drawing;!$view.drawer -and $view.setup.paper.transparent -and !$view.sheet.shown -and $view.page.paper_color -eq $view.copy.transparent} 'Use Color did not choose a transparent sheet'
    New-Capture 'main-transparent'
    Open-Drawer 'new-drawing-paper-color' 'paper_color'
    Press 'new-drawing-color-mode-color';Press 'new-drawing-paper-color-0'
    Wait-Until {$colors=Drawer;!$colors.transparent -and $colors.palette[0].selected} 'The sheet did not return to a color'
    Press 'new-drawing-color-use'
    Wait-Until {$view=Drawing;!$view.drawer -and !$view.setup.paper.transparent -and $view.sheet.shown} 'Use Color did not restore a visible sheet'
    Type-Text 'document-resolution' '150' -Commit
    Wait-Until {(Drawing).setup.resolution -eq 150} 'Typed resolution did not apply'
    Pick-Choice 'new-drawing-unit' (Value-Index (Drawing).units 'px')
    Wait-Until {$view=Drawing;$view.setup.unit -eq 'px' -and (Field-Value 'document-width') -eq $view.values[0]} 'Pixels did not apply'
    Type-Text 'document-width' '64*2' -Commit;Wait-Until {(Drawing).values[0] -eq 128} 'The width expression did not apply'
    Type-Text 'document-height' '96' -Commit;Wait-Until {(Drawing).values[1] -eq 96} 'The height did not apply'
    Press 'new-drawing-link';Wait-Until {(Drawing).linked} 'Link did not apply'
    Type-Text 'document-width' '256' -Commit;Wait-Until {$values=(Drawing).values;$values[0] -eq 256 -and $values[1] -eq 192} 'Linked sizes did not keep the proportions'
    Press 'new-drawing-link';Wait-Until {!(Drawing).linked} 'Unlink did not apply'
    Type-Text 'document-width' '128' -Commit;Type-Text 'document-height' '96' -Commit
    Wait-Until {$values=(Drawing).values;$values[0] -eq 128 -and $values[1] -eq 96} 'Unlinked sizes did not apply'
    Press 'new-drawing-portrait';Wait-Until {$view=Drawing;$view.page.orientation -eq 'portrait' -and $view.values[0] -eq 96} 'Portrait did not turn the sheet'
    Press 'new-drawing-landscape';Wait-Until {$view=Drawing;$view.page.orientation -eq 'landscape' -and $view.values[0] -eq 128} 'Landscape did not turn the sheet'
    $view=Drawing
    Pick-Choice 'document-depth' (Value-Index $view.depths 'F16')
    Wait-Until {$view=Drawing;$view.setup.blend_space -eq 'Linear' -and $view.blending_note -and !(Owned 'document-blending').Current.IsEnabled} 'Float did not force Linear blending'
    Pick-Choice 'document-depth' (Value-Index $view.depths 'U8')
    Wait-Until {$view=Drawing;$view.setup.blend_space -eq 'Perceptual' -and !$view.blending_note -and (Owned 'document-blending').Current.IsEnabled} 'Leaving Float lost the Perceptual blending choice'
    Type-Text 'document-width' '2+'
    Press 'new-drawing-create';Start-Sleep -Milliseconds 800
    if(!(Drawing) -or [string]::IsNullOrWhiteSpace((Owned 'document-width').Current.HelpText) -or (Model).state.document_file.epoch -ne 0){throw 'An invalid size expression did not keep the dialog open with its refusal'}
    Type-Text 'document-width' '128' -Commit;Wait-Until {[string]::IsNullOrWhiteSpace((Owned 'document-width').Current.HelpText)} 'The corrected width kept its refusal'
    Type-Text 'new-drawing-name' 'Harbor study'
    Wait-Until {(Drawing).name -eq 'Harbor study'} 'The drawing name did not reach the dialog'
    $count=@((Model).windows_tabs.tabs).Count
    Press 'new-drawing-create'
    Wait-Until {$model=Model;!$model.state.new_drawing -and @($model.windows_tabs.tabs).Count -eq $count+1 -and !$model.state.document_file.busy} 'Create did not open the new drawing' 60
    Wait-Until {$model=Model;@($model.windows_tabs.tabs|Where-Object id -eq $model.windows_tabs.selected)[0].title -eq 'Harbor study'} 'The new drawing did not take its name'
    Wait-Until {(Model).state.tabs[0].width -eq 128 -and (Model).state.tabs[0].height -eq 96} 'New drawing did not use the chosen size'
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
    $script:scope=Named-Control $title ([System.Windows.Automation.ControlType]::Window)
}
function Picker([string]$Name) {
    $script:scope=$root
    $script:scope=Named-Control $Name ([System.Windows.Automation.ControlType]::Window)
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
function Write-PreservedPackage([string]$Destination,[byte[]]$Preview) {
    $members=[CapyPackageFixture]::Read((Join-Path $repo 'crates/layer-core/src/package/codec/fixtures/authored-filters.capy'))
    $manifest=[Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json
    @($manifest.objects|Where-Object type -eq 'capy.occurrence/3')[0].data|Add-Member -Force blend 'future-package-blend'
    foreach($record in @($manifest.objects|Where-Object type -eq 'capy.output/2')){
        $record.data|Add-Member -Force name 'Package preview output'
        $record.data.PSObject.Properties.Remove('representation')
        if($record.id -eq $manifest.default_output.ref){$record.data|Add-Member representation ([ordered]@{member='preview.png';size=@(2,1);color='srgb'})}
    }
    $archive=[Collections.Generic.List[Collections.Generic.KeyValuePair[string,byte[]]]]::new()
    foreach($member in $members){
        if($member.Key -eq 'preview.png'){continue}
        $bytes=if($member.Key -eq 'manifest.json'){[Text.Encoding]::UTF8.GetBytes(($manifest|ConvertTo-Json -Depth 100 -Compress))}else{$member.Value}
        $archive.Add([Collections.Generic.KeyValuePair[string,byte[]]]::new($member.Key,$bytes))
    }
    $archive.Add([Collections.Generic.KeyValuePair[string,byte[]]]::new('preview.png',$Preview))
    [CapyPackageFixture]::Write($Destination,$archive)
}
function Package-View {
    $script:scope=$root
    Wait-Until {(Model).windows_document.type -eq 'package' -and (Find-Id 'package-view')} 'The package view did not appear' 45
    (Model).windows_document.summary
}
function Package-Action([string]$Name) {
    $script:scope=Find-Id 'package-view'
    try{Invoke-Control $Name}finally{$script:scope=$root}
}
function Stroke-InkPixels {
    $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try {
        $rect=[CapyDocumentControls+Rect]::new()
        if(![CapyDocumentControls]::GetClientRect($drawing.Handle,[ref]$rect)){throw 'No canvas bounds'}
        $bitmap=[Drawing.Bitmap]::new($rect.right,$rect.bottom)
        try {
            $graphics=[Drawing.Graphics]::FromImage($bitmap)
            try{
                $origin=[CapyDocumentControls+Point]::new()
                if(![CapyDocumentControls]::ClientToScreen($drawing.Handle,[ref]$origin)){throw 'No canvas origin'}
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
    $patterns=@(foreach($action in $Queued){
        $script:replayPattern=$null
        Wait-Until {
            $item=Find-Name $action
            if(!$item -or !$item.Current.IsEnabled){return $false}
            $script:replayPattern=$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
            $true
        } "Could not invoke control: $action"
        $script:replayPattern
    })
    $before=@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing').Count
    Invoke-Control 'Test GPU loss'
    if(!$Queued.Count){return}
    Wait-Until {@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing').Count -gt $before} 'GPU reconstruction did not start' 30
    $start=@(Recovery-Events|Where-Object event -eq 'gpu_recovery_preparing')[-1].time
    foreach($pattern in $patterns){$pattern.Invoke()}
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
    Bind-ReviewWindow 'No GPU save review window'
    # GPU readiness can precede the asynchronous restoration of saved Zen/layout.
    Wait-Until {$model=Model;$model.brush_ready -and $model.windows_workspace.ready} 'GPU save review did not restore its workspace' 60
    if((Model).state.workspace.zen_mode){
        (Find-Id 'zen-capy').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
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
        (Control "layer-$paint-content" -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $paint} 'Ink layer did not select'
    }
    if(!(Model).state.layer_tools.controls.alpha_lock){throw 'Controlled stroke target is not editable paint'}
    if(!((Model).state.commands|Where-Object id -eq 'pen').selected){
        $tile=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.resolved_control.command -eq 'pen'}|Select-Object -First 1
        if(!$tile){throw 'The shared toolbar has no Pen command tile'}
        (Control "tile-toolbar-$($tile.id)" -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    Wait-Until {(Model).brush_ready -and ((Model).state.commands|Where-Object id -eq 'pen').selected} 'Pen did not become ready' 45
    if((Model).state.layer_tools.mask_editing -or (Model).state.layer_tools.quick_mask -or (Model).state.layer_tools.editing_layer.mask_selected){throw 'Controlled stroke is editing a mask'}
    $script:drawIndex++
    $channel=(.02+.02*$script:drawIndex).ToString('R',[Globalization.CultureInfo]::InvariantCulture)
    Set-InkColor @('.03',$channel,'.025')
    $revision=(Model).state.document_file.revision
    Invoke-Control 'Test pen'
    Wait-Until {(Model).state.document_file.modified -and (Model).state.document_file.revision -gt $revision} 'Controlled stroke did not modify the drawing'
}
function Set-InkColor([string[]]$Channels) {
    $colorTile=((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'}|Select-Object -First 1
    if(!$colorTile){throw 'Controlled stroke has no color toolbar tile'}
    (Control "tile-toolbar-$($colorTile.id)" -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {Find-Id 'tool-drawer'} 'Controlled ink drawer did not open'
    (Control 'color-foreground' -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Model).state.colors.slot -eq 'foreground'} 'Foreground paint did not select'
    (Control 'color-edit' -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    (Control 'edit-color-form-0').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Choose-Item (Control 'edit-color-form-0-linear_rgb')
    Wait-Until {(Find-Id 'edit-color-form-0').Current.Name -eq 'Linear RGB'} 'Controlled ink did not switch to Linear RGB'
    for($i=0;$i -lt 3;$i++){
        (Control "edit-color-0-$i").GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        $entry=Control "edit-color-0-$i-entry";$entry.SetFocus()
        $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($channels[$i])
        [CapyRowPointer]::Key([uint32]$review.Id,0x0D)
        Wait-Until {!(Find-Id "edit-color-0-$i-entry")} 'Controlled ink value did not commit'
    }
    (Control 'edit-color-apply').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$definition=(Model).paint_pair.definition;$definition.linear_rgb -and $definition.rgba[3] -eq 1 -and @((0..2)|Where-Object {[Math]::Abs($definition.linear_rgb[$_]-[double]$Channels[$_]) -ge .000001}).Count -eq 0} 'Controlled ink color did not publish'
    Wait-Until {!(Find-Id 'edit-color-apply')} 'Controlled ink editor did not close'
    (Control "tile-toolbar-$($colorTile.id)" -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find-Id 'tool-drawer')} 'Controlled ink drawer did not close'
}
function Draw-Below {
    $script:scope=$root
    $camera=(Model).state.camera;$bounds=(Find-Id 'drawing-canvas').Current.BoundingRectangle
    $revision=(Model).state.document_file.revision
    $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try{
        [CapyRowPointer]::SetForegroundWindow($drawing.Handle)|Out-Null
        [CapyRowPointer]::Initialize([uint32]$review.Id)
        try{
            for($i=0;$i -le 16;$i++){
                $x=[int]($bounds.X+$camera.translation[0]+$camera.zoom*(20+5*$i));$y=[int]($bounds.Y+$camera.translation[1]+$camera.zoom*(78+[Math]::Sin($i/3)))
                if($i -eq 0){[CapyRowPointer]::Down('mouse',$x,$y)}else{[CapyRowPointer]::Move($x,$y)}
                Start-Sleep -Milliseconds 12
            }
            [CapyRowPointer]::Up()
        }finally{[CapyRowPointer]::Dispose()}
    }finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
    Wait-Until {(Model).state.document_file.revision -gt $revision} 'The stroke below the controlled stroke did not modify the drawing'
}
function Test-LightNewDrawing {
    $env:CAPY_STORAGE_DIR=Join-Path $run 'light-profile'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme='light'}|ConvertTo-Json -Depth 4))
    $script:stderr=Join-Path $run 'light.stderr.log'
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
    Write-Output "Light New drawing review process $($review.Id)"
    Bind-ReviewWindow 'No light review window'
    Wait-Until {$model=Model;$model.brush_ready -and $model.state.theme -eq 'light'} 'Light review did not start' 45
    New-Journey 'light'
    if($Journey -eq 'full'){Color-Workflows}
    Request-Close;Wait-Closed 'Light review close did not finish'
    if((Get-Item -LiteralPath $stderr).Length){throw 'Light native review reported stderr'}
}
Bind-ReviewWindow 'No review window'
Wait-Until {(Model).brush_ready} 'Canvas startup did not finish' 45
if(!(Model).windows_isolated_settings){throw 'Review must use an isolated profile'}
if($focusedColor){
    Open-NewDrawing
    Pick-Choice 'new-drawing-unit' (Value-Index (Drawing).units 'px')
    if((Drawing).linked){Press 'new-drawing-link';Wait-Until {!(Drawing).linked} 'Focused drawing did not unlink its dimensions'}
    Type-Text 'document-width' '128' -Commit;Type-Text 'document-height' '96' -Commit
    Type-Text 'new-drawing-name' 'Workflow study'
    Wait-Until {(Drawing).name -eq 'Workflow study' -and (Drawing).values[0] -eq 128 -and (Drawing).values[1] -eq 96} 'Focused drawing setup did not acknowledge its values'
    $count=@((Model).windows_tabs.tabs).Count;Press 'new-drawing-create'
    Wait-Until {$model=Model;!$model.state.new_drawing -and @($model.windows_tabs.tabs).Count -eq $count+1 -and !$model.state.document_file.busy} 'Focused drawing did not open' 60
}elseif(!$nativeWriteback){New-Journey 'dark' -Full}
if($Journey -eq 'new-drawing'){
    Request-Close;Wait-Closed 'New drawing review close did not finish'
    if((Get-Item -LiteralPath $stderr).Length){throw 'New drawing native review reported stderr'}
    Test-LightNewDrawing
    @{journey=$Journey;new_drawing_captures='dark and light';evidence=$run;scope='New drawing presets, paper and color drawers, texture visibility, sheet creation and owned close'}|ConvertTo-Json
    return
}
function Combo-Value($Combo){$selection=$Combo.GetCurrentPattern([System.Windows.Automation.SelectionPattern]::Pattern).Current.GetSelection();if($selection.Count){$selection[0].Current.Name}}
function Combo-Select($Combo,[scriptblock]$Pick){
    $Combo.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $hit=@{item=$null};Wait-Until {$hit.item=@($Combo.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new(
        [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))|Where-Object $Pick)[0];$hit.item} 'Missing drop-down choice'
    $hit.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}


function Color-Command([string]$Id){
    $script:scope=$root
    Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Color command stayed disabled: $Id" 45
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Image'
    $menu=Named-Control 'Color Management' ([System.Windows.Automation.ControlType]::MenuItem)
    $menu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Wait-Until {$item=Find-Id $Id;$item -and $item.Current.IsEnabled} "Color command did not open: $Id"
    (Find-Id $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Color-Preview([string]$Space,[switch]$Copy){
    Wait-Until {$view=(Model).windows_document;$view.kind -eq 'convert' -and $view.stage -eq 'preview' -and !$view.error -and $view.details.result.space -eq $Space -and [bool]$view.details.copy -eq [bool]$Copy} 'Automatic color comparison did not settle on the latest choice' 60
    $confirm=if($Copy){(Model).windows_document.details.feature_copy.color.create_copy}else{'Apply'}
    Wait-Until {$dialog=Find-Id 'document-workflow';$dialog -and (Find-Name $confirm).Current.IsEnabled} 'Completed comparison did not enable its commit action'
}
function Color-Workflows {
    $script:scope=$root;$before=Model;$original=$before.windows_tabs.selected;$count=@($before.windows_tabs.tabs).Count
    $layers=$before.state.layers|ConvertTo-Json -Depth 40 -Compress;$file=$before.state.document_file|Select-Object revision,modified,location,unsaved_name,epoch|ConvertTo-Json -Depth 20 -Compress
    Color-Command 'convert_color_space'
    Wait-Until {$view=(Model).windows_document;$view.kind -eq 'convert' -and $view.stage -eq 'preview'} 'Color comparison did not start automatically' 60
    foreach($space in @('Display P3','sRGB','Display P3')){Combo-Select (Owned 'document-profile-choice') {$_.Current.Name -eq $space}}
    Color-Preview 'DisplayP3'
    if(((Model).state.document_file|Select-Object revision,modified,location,unsaved_name,epoch|ConvertTo-Json -Depth 20 -Compress) -ne $file){throw 'Comparison choices edited the original drawing'}
    Invoke-Control 'Cancel';Wait-Until {!(Model).windows_document -and !@((Model).state.requests).Count} 'Comparison Cancel did not drain the worker'
    Color-Command 'convert_color_space'
    Wait-Until {(Find-Id 'document-profile-choice')} 'Color choices did not reopen'
    Combo-Select (Owned 'document-profile-choice') {$_.Current.Name -eq 'Display P3'}
    Color-Preview 'DisplayP3'
    $caption=(Model).windows_document.details.feature_copy.color.create_flattened
    Choose-Item (Named-Control $caption ([System.Windows.Automation.ControlType]::CheckBox))
    Color-Preview 'DisplayP3' -Copy
    Capture ('automatic-color-copy-preview-'+(Model).state.theme) -WithModel
    Invoke-Control (Model).windows_document.details.feature_copy.color.create_copy
    Wait-Until {$view=Model;!$view.windows_document -and !$view.state.document_file.busy -and @($view.windows_tabs.tabs).Count -eq $count+1 -and $view.windows_tabs.selected -ne $original} 'Flattened conversion did not open a new drawing' 60
    if(!@((Model).windows_tabs.tabs|Where-Object id -eq $original).Count){throw 'Flattened copy removed its source drawing'}
    $modified=(Model).state.document_file.modified
    File-Command 'close_document'
    if($modified){Confirm-Dialog;Invoke-Control 'Discard Changes';$script:scope=$root}
    Wait-Until {$view=Model;!$view.windows_document -and @($view.windows_tabs.tabs).Count -eq $count -and $view.windows_tabs.selected -eq $original} 'Closing the copy did not restore its source drawing' 60
    if(((Model).state.layers|ConvertTo-Json -Depth 40 -Compress) -ne $layers){throw 'Flattened copy changed source layers'}
    File-Command 'document_properties'
    Wait-Until {(Model).windows_document.kind -eq 'properties'} 'Document Properties did not open'
    $appearance=Control 'PrimaryButton' -Within (Control 'document-workflow') -Arranged
    if($appearance.Current.Name -ne (Model).windows_document.details.proof){throw 'Properties primary action is not Appearance'}
    $appearance.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Model).windows_document.kind -eq 'proof' -and (Find-Id 'proof-profile')} 'Properties Appearance did not open proof setup'
    Invoke-Control 'Cancel';Wait-Until {!(Model).windows_document -and !@((Model).state.requests).Count} 'Appearance cancellation did not close'
}
function Writeback-Tiles([string]$Path,[string]$Mode) {
    $members=[CapyPackageFixture]::Read($Path)
    $manifest=[Text.Encoding]::UTF8.GetString(@($members|Where-Object Key -eq 'manifest.json')[0].Value)|ConvertFrom-Json
    $paint=@($manifest.objects|Where-Object {$_.type -eq 'capy.paint-source/2' -and $_.data.tiles -and @($_.data.tiles).Count})
    if($paint.Count -ne 1 -or $paint[0].data.color_mode -ne $Mode -or ($paint[0].data.domain.size -join ',') -ne '768,256'){throw 'Saved native paint source has the wrong mode or domain'}
    $tiles=@($paint[0].data.tiles|Sort-Object {$_.coordinate[0]})
    if($tiles.Count -ne 3 -or (($tiles|ForEach-Object {$_.coordinate -join ','}) -join ';') -ne '0,0;1,0;2,0'){throw 'Native stroke did not publish all three tile columns'}
    @($tiles|ForEach-Object {
        $tile=$_;$resources=@($manifest.resources|Where-Object id -eq $tile.resource.ref)
        if($resources.Count -ne 1){throw 'Native tile resource is missing or ambiguous'}
        $resource=$resources[0];$data=$resource.data
        if($tile.plane -ne 'color' -or $resource.type -ne 'capy.raster-tile/1' -or $resource.encoding -ne 'capy.lz4-tile/1' -or $data.channels -ne 'gray_alpha' -or $data.depth -ne 'u8' -or $data.alpha -ne 'straight' -or $data.transfer -ne 'profile'){throw 'Saved native tile has the wrong sample descriptor'}
        $packs=@($members|Where-Object Key -eq $resource.location.pack)
        if($packs.Count -ne 1){throw 'Native tile pack is missing or ambiguous'}
        $offset=[long]$resource.location.offset;$length=[long]$resource.bytes;$bytes=$packs[0].Value
        if($offset -lt 0 -or $length -le 0 -or $offset -gt $bytes.LongLength -or $length -gt $bytes.LongLength-$offset){throw 'Native tile byte range leaves its pack'}
        $hash=[Security.Cryptography.SHA256]::Create()
        try{$digest=[BitConverter]::ToString($hash.ComputeHash($bytes,[int]$offset,[int]$length)).Replace('-','')}finally{$hash.Dispose()}
        [ordered]@{coordinate=@($tile.coordinate);plane=$tile.plane;descriptor=$data;encoding=$resource.encoding;bytes=$length;sha256=$digest}
    })
}
function Writeback-Command([string]$Menu,[string]$Id) {
    $script:scope=$root
    Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Writeback command stayed disabled: $Id" 45
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
    Press $Id
}
function Writeback-Panel([string]$Id) {
    if(!@((Model).layout.groups|Where-Object {$_.panels -contains $Id}).Count){
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
        $menu=@((Model).application_menus|Where-Object id -eq 'window')[0].model.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.panel -eq $Id}
        if(@($menu).Count -ne 1){throw "Shared Window menu has no unique panel action: $Id"}
        (Control $menu.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {@((Model).layout.groups|Where-Object {$_.panels -contains $Id}).Count} "Native writeback panel did not open: $Id"
    }
    if(!@((Model).layout.groups|Where-Object active -eq $Id).Count){Press "panel-tab-$Id"}
    Wait-Until {@((Model).layout.groups|Where-Object active -eq $Id).Count -eq 1} "Native writeback panel did not activate: $Id"
}
function Writeback-Ready {
    Wait-Until {$view=Model;$canvas=Find-Id 'drawing-canvas';$view.canvas_ready -and $view.brush_ready -and !$view.state.document_file.busy -and $canvas -and $canvas.Current.IsEnabled -and !$canvas.Current.IsOffscreen} 'Native writeback drawing did not become ready' 60
    $view=Model
    if($view.error -or $view.state.host_error -or $view.windows_gpu_generation -ne $script:writebackGpu){throw 'Native writeback changed the GPU owner or reported an editor error'}
}
function Writeback-State {
    $view=Model
    [ordered]@{file=$view.state.document_file;layers=$view.state.layers;editing_layer=$view.state.layer_tools.editing_layer.id;mode=@($view.state.layer_properties.controls|Where-Object key -eq 'color_mode')[0].value.value;history=@($view.state.commands|Where-Object {$_.id -in @('undo','redo')}|Select-Object id,enabled);gpu_generation=$view.windows_gpu_generation}
}
function Writeback-Projection {
    if($root.Current.ProcessId -ne $review.Id -or [long]$root.Current.NativeWindowHandle -ne $drawing.Handle.ToInt64()){throw 'Native writeback projection does not belong to the owned drawing'}
    $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1] -or $camera.viewport[0] -le 0 -or $camera.viewport[1] -le 0){throw 'Native writeback requires an unrotated canvas projection'}
    [ordered]@{bounds=@($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);zoom=$camera.zoom;translation=@($camera.translation);viewport=@($camera.viewport);work_area=@($camera.work_area);process_id=$review.Id;hwnd=$drawing.Handle.ToInt64();camera=$camera}
}
function Writeback-Geometry($Projection) {[ordered]@{bounds=$Projection['bounds'];zoom=$Projection['zoom'];translation=$Projection['translation'];viewport=$Projection['viewport'];work_area=$Projection['work_area']}}
function Writeback-Point([double]$X,[double]$Y,$p) {
    @([int]($p.bounds[0]+($p.translation[0]+$p.zoom*$X)*$p.bounds[2]/$p.viewport[0]),[int]($p.bounds[1]+($p.translation[1]+$p.zoom*$Y)*$p.bounds[3]/$p.viewport[1]))
}
function Writeback-Checked([scriptblock]$Action) {
    Foreground
    $window=$root.Current.BoundingRectangle;[CapyRowPointer]::Hover([int]($window.X+20),[int]($window.Y+20))
    $projection=Writeback-Projection
    if((Writeback-Geometry $projection|ConvertTo-Json -Depth 8 -Compress) -cne $script:writebackProjection){throw 'Native writeback pixel projection changed'}
    $signature=$projection|ConvertTo-Json -Depth 20 -Compress
    try{& $Action $projection}finally{
        if((Writeback-Projection|ConvertTo-Json -Depth 20 -Compress) -cne $signature){throw 'Native writeback owned projection changed during sampling or input'}
    }
}
function Writeback-Pixels([string]$Name,$Projection) {
    (@(foreach($x in @(128,384,640)){
        $point=Writeback-Point $x 128 $Projection;$path=if($Name){Join-Path $run "$Name-$x.png"}else{''}
        Screen-Pixels $point[0] $point[1] $path
    }) -join ',')
}
function Stable-WritebackPixels([string]$Name='') {Writeback-Checked {param($p) Wait-StablePixels {Writeback-Pixels $Name $p}}}
function Writeback-Stroke([string]$Device) {
    Writeback-Checked {param($p)
        $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
        try{
            [CapyRowPointer]::Initialize([uint32]$review.Id)
            try{
                $point=Writeback-Point 96 128 $p;[CapyRowPointer]::Down($Device,$point[0],$point[1])
                try{foreach($x in 1..32){$point=Writeback-Point (96+18*$x) 128 $p;[CapyRowPointer]::Move($point[0],$point[1]);Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
                [CapyRowPointer]::Verify()
            }finally{[CapyRowPointer]::Dispose()}
        }finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
    }
}
function Native-Writeback {
    $script:writebackGpu=(Model).windows_gpu_generation
    $cases=@(foreach($choice in @(@('Grayscale',1,'grayscale'),@('Two-tone (black & white)',2,'two_tone'))){foreach($device in @('mouse','pen')){
        $name="$($choice[2])-$device";$file=Join-Path $run "$name 日本語.capy";$resaved=Join-Path $run "$name-reopened.capy"
        Open-NewDrawing
        Pick-Choice 'new-drawing-unit' (Value-Index (Drawing).units 'px')
        if((Drawing).linked){Press 'new-drawing-link';Wait-Until {!(Drawing).linked} 'Writeback drawing did not unlink its dimensions'}
        Type-Text 'document-width' '768' -Commit;Type-Text 'document-height' '256' -Commit
        Pick-Choice 'document-depth' (Value-Index (Drawing).depths 'U8')
        Type-Text 'new-drawing-name' $name
        Wait-Until {(Drawing).name -eq $name -and (Drawing).values[0] -eq 768 -and (Drawing).values[1] -eq 256} 'Writeback drawing setup did not acknowledge its values'
        $count=@((Model).windows_tabs.tabs).Count;Press 'new-drawing-create'
        Wait-Until {! (Drawing) -and @((Model).windows_tabs.tabs).Count -eq $count+1} 'Writeback drawing did not open' 60
        Writeback-Ready
        Press (Tool-Tile 'pen')
        Writeback-Panel 'tool_settings'
        $subtools=@((Model).state.tool_set.subtools);$gpen=@(0..($subtools.Count-1)|Where-Object {$subtools[$_].preview -eq 1})
        if($gpen.Count -ne 1){throw 'The native Pen group has no unique G-Pen'}
        Press "tool-subtool-$($gpen[0])"
        Wait-Until {(Model).state.brush.preset -eq 1 -and (Model).brush_ready} 'G-Pen did not prepare for native writeback' 60
        Type-Text 'tool-setting-size' '160' -Commit
        Wait-Until {[Math]::Abs((Model).state.brush.diameter-160) -lt .001} 'Native writeback brush size did not commit'
        Set-InkColor @('0','0','0')
        Writeback-Panel 'properties'
        Combo-Select (Owned 'property-color_mode') {$_.Current.Name -eq $choice[0]}
        Wait-Until {@((Model).state.layer_properties.controls|Where-Object key -eq 'color_mode')[0].value.value -eq $choice[1]} 'Native writeback color mode did not apply'
        Writeback-Command 'View' 'fit_canvas';Writeback-Ready
        $projection=Writeback-Projection;$script:writebackProjection=Writeback-Geometry $projection|ConvertTo-Json -Depth 8 -Compress
        $area=[Windows.Rect]::new(($projection.bounds[0]+$projection.work_area[0]*$projection.bounds[2]/$projection.viewport[0]),($projection.bounds[1]+$projection.work_area[1]*$projection.bounds[3]/$projection.viewport[1]),($projection.work_area[2]*$projection.bounds[2]/$projection.viewport[0]),($projection.work_area[3]*$projection.bounds[3]/$projection.viewport[1]))
        foreach($x in @(128,384,640)){$point=Writeback-Point $x 128 $projection;if(!$area.Contains([Windows.Rect]::new($point[0]-48,$point[1]-48,96,96))){throw 'Native writeback tile sample leaves the clear canvas work area'}}
        $before=Writeback-State;$baseline=Stable-WritebackPixels
        Writeback-Stroke $device
        Wait-Until {$view=Model;$view.state.document_file.revision -gt $before.file.revision -and $view.state.document_file.modified -and @($view.state.layers|Where-Object id -eq $before.editing_layer)[0].paint_revision -ne @($before.layers|Where-Object id -eq $before.editing_layer)[0].paint_revision} 'Native continuous stroke did not commit' 45
        Writeback-Ready;$painted=Stable-WritebackPixels;$after=Writeback-State
        for($i=0;$i -lt 3;$i++){if($painted.Split(',')[$i] -eq $baseline.Split(',')[$i]){throw "Native $name stroke did not change tile column $i"}}
        Capture $name -WithModel -Composed
        Writeback-Command 'Edit' 'undo'
        Writeback-Checked {param($p) Wait-Until {(Model).state.document_file.revision -gt $after.file.revision -and (Model).state.document_file.modified -eq $before.file.modified -and (Writeback-Pixels '' $p) -eq $baseline} 'One Undo did not restore all native writeback baseline pixels' 45}
        $undone=Writeback-State
        Writeback-Command 'Edit' 'redo'
        Writeback-Checked {param($p) Wait-Until {(Model).state.document_file.revision -gt $undone.file.revision -and (Writeback-Pixels '' $p) -eq $painted} 'One Redo did not restore all native writeback stroke pixels' 45}
        $redone=Writeback-State
        File-Command 'save_document';Picker 'Save As';Choose-Path $file;Idle
        Wait-Until {(Test-Path -LiteralPath $file) -and (Model).state.document_file.location.uri -eq $file -and !(Model).state.document_file.modified} 'Native writeback Save did not establish a clean checkpoint' 60
        $saved=Writeback-State;$tiles=@(Writeback-Tiles $file $choice[2]);$fingerprint=$tiles|ConvertTo-Json -Depth 12 -Compress
        if($saved.file.revision -ne $redone.file.revision -or (Writeback-Checked {param($p) Writeback-Pixels '' $p}) -ne $painted){throw 'Saving native writeback changed its history or canvas pixels'}
        File-Command 'close_document';Idle
        Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count} 'Saved native writeback drawing did not close' 45
        $epoch=[Math]::Max((Model).state.document_file.epoch,$saved.file.epoch)
        File-Command 'open_document';Picker 'Open';Choose-Path $file;Idle
        Wait-Until {(Model).state.document_file.epoch -gt $epoch -and (Model).state.document_file.location.uri -eq $file -and !(Model).state.document_file.modified} 'Native writeback Open did not admit a fresh clean drawing' 60
        Writeback-Ready
        Writeback-Panel 'properties'
        Wait-Until {@((Model).state.layer_properties.controls|Where-Object key -eq 'color_mode')[0].value.value -eq $choice[1]} 'Reopened native writeback lost its color mode'
        Writeback-Command 'View' 'fit_canvas';Writeback-Ready
        if((Stable-WritebackPixels "$name-reopened") -ne $painted){throw 'Saved native writeback did not reopen with exact canvas pixels'}
        $reopened=Writeback-State
        File-Command 'save_document_as';Picker 'Save As';Choose-Path $resaved;Idle
        Wait-Until {(Model).state.document_file.location.uri -eq $resaved -and !(Model).state.document_file.modified -and (Test-Path -LiteralPath $resaved)} 'Reopened native writeback did not save cleanly' 60
        if((@(Writeback-Tiles $resaved $choice[2])|ConvertTo-Json -Depth 12 -Compress) -cne $fingerprint){throw 'Native writeback reopening changed authored tile descriptors or encoded ranges'}
        Capture "$name-reopened" -WithModel -Composed
        File-Command 'close_document';Idle
        Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count} 'Reopened native writeback drawing did not close'
        [ordered]@{name=$name;device=$device;mode=$choice[2];projection=$projection;before=$before;painted=$after;undone=$undone;redone=$redone;saved=$saved;reopened=$reopened;baseline_pixels=$baseline;painted_pixels=$painted;tiles=$tiles;file=$file;file_sha256=(Get-FileHash -LiteralPath $file).Hash;reopened_file=$resaved;reopened_file_sha256=(Get-FileHash -LiteralPath $resaved).Hash}
    }})
    $result=[ordered]@{journey=$Journey;theme=(Model).state.theme;executable_sha256=(Get-FileHash -LiteralPath $Executable).Hash;dll_sha256=(Get-FileHash -LiteralPath (Join-Path $directory 'layer_windows.dll')).Hash;fixture_sha256=(Get-FileHash -LiteralPath $PSCommandPath).Hash;cases=$cases;evidence=$run}
    [IO.File]::WriteAllText((Join-Path $run 'native-writeback.json'),($result|ConvertTo-Json -Depth 80),[Text.UTF8Encoding]::new($false))
    Request-Close;Wait-Closed 'Native writeback review did not close'
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native writeback review reported stderr'}
    $result|ConvertTo-Json -Depth 80
}
if($nativeWriteback){
    $context=[CapyDocumentControls]::SetThreadDpiAwarenessContext([IntPtr](-4))
    try{Native-Writeback}finally{[CapyDocumentControls]::SetThreadDpiAwarenessContext($context)|Out-Null}
    return
}
if(!$FailGpu){Color-Workflows}
if($focusedColor){
    $moveTile=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.resolved_control.command -eq 'move'}|Select-Object -First 1
    if(!$moveTile){throw 'The shared toolbar has no Move command tile'}
    (Control "tile-toolbar-$($moveTile.id)" -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {((Model).state.commands|Where-Object id -eq 'move').selected} 'Move did not acknowledge before the controlled Pen switch'
    Draw;Capture ('color-workflows-'+(Model).state.theme) -WithModel
    Request-Close;Wait-Closed 'Focused workflow drawing did not close'
    @{journey=$Journey;automatic_comparison_cancel_latest_choice='passed';flattened_copy_new_drawing_and_source_retention='passed';properties_appearance='passed';controlled_move_to_pen_color_and_stroke='passed';evidence=$run}|ConvertTo-Json
    return
}

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
Wait-Until {@((Model).state.layers).Count -eq 3 -and (Model).state.layer_tools.editing_layer.object} 'Image layer was not selected'
if((Model).error){throw 'Successful import did not clear the previous decoder error'}
$imported=(Model).state.layer_tools.editing_layer.id
Wait-Until {$layer=@((Model).state.layers|Where-Object id -eq $imported)[0];$layer.object -and $layer.label -eq 'Source image 日本語'} 'The imported image layer was not named after its file'
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
if($png.Length -lt 45 -or [BitConverter]::ToString($png[0..7]).Replace('-','') -ne '89504E470D0A1A0A' -or
    [BitConverter]::ToString($png[12..23]).Replace('-','') -ne '494844520000008000000060' -or
    [BitConverter]::ToString($png[($png.Length-8)..($png.Length-1)]).Replace('-','') -ne '49454E44AE426082'){
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
$exportedHash=(Get-FileHash -LiteralPath $exported).Hash
Export-Again
if((Get-FileHash -LiteralPath $exported).Hash -ne $exportedHash){throw 'Export Again did not rewrite identical pixels'}
$inkBefore=Stroke-InkPixels
Draw-Below
Export-Again
if((Get-FileHash -LiteralPath $exported).Hash -eq $exportedHash){throw 'Export Again did not export the current pixels'}
if((Stroke-InkPixels) -ne $inkBefore){throw 'The Export Again stroke reached the controlled stroke region'}
if(!(Model).state.document_file.modified -or (Model).state.document_file.location.uri -ne $first){throw 'Export Again acknowledged a project save'}
Remove-Item -LiteralPath $exported
$script:scope=$root;File-Command 'export_again';Picker 'Save As';$moved=Join-Path $run 'Export again.png';Choose-Path $moved;Idle
Wait-Until {(Test-Path -LiteralPath $moved) -and (Model).state.document_file.export_uri -eq $moved} 'Export Again did not fall back to choosing a new destination'
$revision=(Model).state.document_file.revision
$script:scope=$root;Invoke-Control 'Undo'
Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'Undo did not remove the Export Again stroke'
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
        foreach($field in @('revision','export_uri')){$state.document_file.PSObject.Properties.Remove($field)}
        @($state.document_file,$state.camera,$state.workspace,$state.brush)|ConvertTo-Json -Depth 80 -Compress
    }
    $png={
        Combo-Select (Find-Id 'export-format') {$_.Current.Name -like 'PNG*'}
        Wait-Until {$format=Find-Id 'export-format';$format -and (Combo-Value $format) -like 'PNG*'} 'Export did not choose PNG'
    }
    $reference=Join-Path $run 'Recovered-0.png'
    $script:scope=$root;File-Command 'export_document' $png;Picker 'Save As';Choose-Path $reference;Idle
    Wait-Until {Test-Path -LiteralPath $reference} 'The drawing did not export before GPU recovery'
    $exportHash=(Get-FileHash -LiteralPath $reference -Algorithm SHA256).Hash
    $beforeRecovery=Signature
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
        File-Command 'export_document' $png;Picker 'Save As';Choose-Path $restored;Idle
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
$preserved=Join-Path $run 'Preserved package.capy'
$previewImage=[Drawing.Bitmap]::new(2,1,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{$previewImage.SetPixel(0,0,[Drawing.Color]::FromArgb(255,16,32,48));$previewImage.SetPixel(1,0,[Drawing.Color]::FromArgb(255,16,32,48));$stream=[IO.MemoryStream]::new();$previewImage.Save($stream,[Drawing.Imaging.ImageFormat]::Png);$previewBytes=$stream.ToArray()}finally{$previewImage.Dispose()}
Write-PreservedPackage $preserved $previewBytes
$packageHash=(Get-FileHash -LiteralPath $preserved).Hash
File-Command 'open_document';Picker 'Open';Choose-Path $preserved
$summary=Package-View
if(!$summary.capabilities.export -or $summary.capabilities.edit -or $summary.capabilities.save){throw 'The package view offered the wrong actions'}
Capture 'package-view' -WithModel
Package-Action $summary.copy_original
$copy=Join-Path $run 'Copied package.capy'
Picker 'Save As';Choose-Path $copy
$summary=Package-View;Idle
Wait-Until {try{(Test-Path -LiteralPath $copy) -and (Get-FileHash -LiteralPath $copy -ErrorAction Stop).Hash -eq $packageHash}catch{$false}} 'Copy Original did not write the package bytes' 20
Package-Action $summary.export_preview
$exported=Join-Path $run 'Package preview.png'
Picker 'Save As';Choose-Path $exported
$summary=Package-View;Idle
Wait-Until {try{(Test-Path -LiteralPath $exported) -and [Convert]::ToBase64String([IO.File]::ReadAllBytes($exported)) -eq [Convert]::ToBase64String($previewBytes)}catch{$false}} 'Export Preview did not write the embedded preview bytes' 20
Package-Action $summary.close
Wait-Until {(Model).windows_document.type -ne 'package'} 'Close did not leave the package view'
if((Model).state.document_file.epoch -ne $epoch -or (Model).state.document_file.location.uri -ne $second){throw 'The package view replaced the live drawing'}
if((Get-FileHash -LiteralPath $preserved).Hash -ne $packageHash){throw 'The package view changed its original'}
Draw
$drawingCount=@((Model).windows_tabs.tabs).Count
Open-NewDrawing;Close-NewDrawing 'new-drawing-cancel';Idle
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
(Named-Control 'Preferences' ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
$script:scope=Named-Control 'Preferences' ([System.Windows.Automation.ControlType]::Window)
$custom=@{item=$null};Wait-Until {
    $custom.item=@($scope.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Custom'))|Where-Object {$_.Current.AutomationId -like 'setting-dark_base-swatch-*'})[0]
    $null -ne $custom.item
} 'Missing custom dark base swatch'
$custom.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
$entry=Named-Control 'Dark theme base color' ([System.Windows.Automation.ControlType]::Edit)
$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#223344')
Request-Close -WithPreferences
Wait-Closed 'Session close did not finish'
if((Get-Item -LiteralPath $stderr).Length){throw 'Native review reported stderr'}
$stderr=Join-Path $run 'reopened.stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
Bind-ReviewWindow 'No restarted review window'
Wait-Until {(Model).brush_ready -and !(Model).windows_recovery.busy -and !(Model).windows_recovery.restoring} 'Session did not reopen' 120
if((Model).state.settings.dark_base -ne '#223344' -or !(Model).state.document_file.modified){throw 'Window close lost the focused Preferences draft or unsaved drawing'}
Request-Close;Wait-Closed 'Restarted session close did not finish'
if((Get-Item -LiteralPath $stderr).Length){throw 'Restarted native review reported stderr'}
$env:CAPY_STORAGE_DIR=Join-Path $run 'untitled-profile'
$stderr=Join-Path $run 'untitled.stderr.log'
$review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
[IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/document-ui-review.pid'),[string]$review.Id)
Write-Output "Untitled close review process $($review.Id)"
Bind-ReviewWindow 'No untitled review window'
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
if(!$RecoverGpu -and !$FailGpu){
    Test-LightNewDrawing
}
if($FailGpu){
    Start-RecoveryReview 'save-as'
    Draw
    $before=Join-Path $run 'Before GPU failure.png'
    File-Command 'export_document';Picker 'Save As';Choose-Path $before;Idle
    Wait-Until {Test-Path -LiteralPath $before} 'Baseline export did not finish'
    $script:scope=$root
    $revision=(Model).state.document_file.revision
    (Find-Id 'zen-capy').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
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
    new_drawing_presets_paper_color_size_name_and_cancel='passed'
    new_drawing_captures=if(!$RecoverGpu -and !$FailGpu){'dark and light'}else{'dark'}
    automatic_color_comparison_cancel_latest_choice_and_copy=if($FailGpu){'not requested without GPU'}else{'passed'}
    properties_appearance=if($FailGpu){'not requested without GPU'}else{'passed'}
    image_picker_draft_cancel_and_error_recovery='passed'
    image_layer_thumbnail_undo_redo_and_embedded_reopen='passed'
    package_view_copy_export_and_close='passed'
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
} catch {
    $failure=$_
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($failure|Format-List * -Force|Out-String)+"`n"+$failure.ScriptStackTrace+"`n"+($failure.InvocationInfo|Format-List * -Force|Out-String))
    if($review -and !$review.HasExited -and $root){try{Capture 'failure' -WithModel}catch{}}
    throw
} finally {
    Exit-CapyEnvironment
}
