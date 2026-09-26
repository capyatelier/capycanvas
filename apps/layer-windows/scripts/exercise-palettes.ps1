param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class CapyPalettePicker {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,IntPtr l,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr h,uint message,UIntPtr w,System.Text.StringBuilder text,uint flags,uint timeout,out UIntPtr result);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint message,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 public static void TypePath(IntPtr edit,uint owner,string path) {
  uint process;GetWindowThreadProcessId(edit,out process);if(process!=owner)throw new Exception("Wrong picker filename owner");
  UIntPtr result;
  if(SendMessageTimeout(edit,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot select picker text");
  if(SendMessageTimeout(edit,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
  foreach(char c in path)if(SendMessageTimeout(edit,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out result)==IntPtr.Zero)throw new Exception("Cannot type picker text");
  var actual=new System.Text.StringBuilder(32768);
  if(SendMessageTimeout(edit,13,new UIntPtr((uint)actual.Capacity),actual,2,2000,out result)==IntPtr.Zero||actual.ToString()!=path)throw new Exception("Picker filename did not match the owned path");
 }
}
"@
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/palettes/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$ControlType=[System.Windows.Automation.ControlType]
function Model {
    try {
        if(!$script:statePath){
            foreach($path in [IO.Directory]::EnumerateFiles($directory,('ui-state-'+$review.Id+'-*.json'))){
                if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
                $value=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$script:statePath=$path;break}
            }
        }
        if($script:statePath){$value=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id){return $value.model}}
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{try{if(& $Condition){return}}catch [System.Windows.Automation.ElementNotAvailableException]{};$review.Refresh();if($review.HasExited){throw 'Owned palettes review exited unexpectedly'};Start-Sleep -Milliseconds 40}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Type,$Within){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $scope=if($Within){$Within}else{$root}
    foreach($entry in $scope.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){if(!$entry.Current.IsOffscreen){return $entry}}
}
function Control([string]$Id,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Type $Type;$null -ne $hit.item} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Center($element){$b=$element.Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {$found.switch=Find ('workspace-switch-'+$Name.ToLowerInvariant());$found.menu=Find 'header-workspace-menu';$found.switch -or $found.menu} "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type $ControlType::MenuItem}
    Wait-Until {try{$switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();$true}catch{$false}} "$Name switch stayed unavailable" 20
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Panel{(Model).palette_panel}
function Swatches{@((Panel).swatches|ForEach-Object {[uint64]$_.id})}
function Menu-Items{
    $items=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::MenuItem))
    @($items|Where-Object {$_.Current.ProcessId -eq $review.Id -and !$_.Current.IsOffscreen})
}
function Close-Menu{
    for($i=0;$i -lt 3 -and @(Menu-Items).Count;$i++){[CapyRowPointer]::Key([uint16]0x1B);Start-Sleep -Milliseconds 150}
    Wait-Until {!@(Menu-Items).Count} 'Menu did not close'
}
function Menu-Invoke([string]$Id){
    $item=@{value=$null};Wait-Until {$item.value=@(Menu-Items)|Where-Object {$_.Current.AutomationId -eq $Id}|Select-Object -First 1;$item.value} "Menu lacks $Id"
    $item.value.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Picker([string]$Title,[string]$Path){
    $dialog=@{value=(Control $Title -Name -Type $ControlType::Window)}
    if($dialog.value.Current.ClassName -ne '#32770' -or $dialog.value.Current.ProcessId -ne $review.Id){throw "$Title picker is outside the owned review"}
    $entry=@{value=$null};Wait-Until {
        $entry.value=$dialog.value.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.OrCondition]::new(
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'),
                [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148')),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
        $null -ne $entry.value
    } "$Title filename field did not appear"
    [CapyPalettePicker]::TypePath([IntPtr]$entry.value.Current.NativeWindowHandle,[uint32]$review.Id,$Path)
    $accept=@{value=$null};Wait-Until {
        $accept.value=$dialog.value.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1'))
        $accept.value -and $accept.value.Current.IsEnabled -and $accept.value.Current.ClassName -eq 'Button'
    } "$Title accept button did not become ready"
    if(![CapyPalettePicker]::PostMessage([IntPtr]$accept.value.Current.NativeWindowHandle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw "Cannot accept the $Title picker"}
}
function Drag($from,$to,[string]$Device='mouse'){
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    try{for($i=1;$i -le 16;$i++){[CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$i/16),[int]($from.y+($to.y-$from.y)*$i/16));Start-Sleep -Milliseconds 16}
        Start-Sleep -Milliseconds 250}finally{[CapyRowPointer]::Up()}
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned palettes review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Palettes review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    foreach($workspace in @(@('Paint','builtin:workspace:illustrator'),@('Photo','builtin:workspace:photographer'))){
        Switch-Workspace $workspace[0] $workspace[1]
        $group=@((Model).layout.groups|Where-Object {$_.panels -contains 'palettes'})[0]
        if(!$group){throw "$($workspace[0]) has no Palettes tab"}
        $index=[Array]::IndexOf(@($group.panels),'color')
        if($index -lt 0 -or $group.panels[$index+1] -ne 'palettes'){throw "$($workspace[0]) does not place Palettes after Color"}
    }
    if(@((Panel).palettes).Count -lt 10){throw 'Starter palettes were not installed'}
    Invoke 'panel-tab-palettes'
    $null=Control 'palettes-panel'
    Wait-Until {@(Swatches).Count -gt 0 -and (Find ('palette-swatch-'+(Swatches)[0]))} 'Saved swatches did not render'
    Capture 'paint-palettes'

    $count=@(Swatches).Count
    Invoke 'palette-add'
    Wait-Until {@(Swatches).Count -eq $count+1} 'The + tile did not save the current color'
    $added=(Swatches)[-1]
    Wait-Until {(Find ('palette-swatch-'+$added)).Current.ItemStatus -eq 'Selected'} 'The added swatch was not selected'
    Invoke 'palette-name'
    $editor=Control 'palette-name-editor' -Type $ControlType::Edit
    $editor.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Fixture ink')
    $editor.SetFocus();[CapyRowPointer]::Key([uint16]0x0D)
    Wait-Until {((Panel).swatches|Where-Object {$_.id -eq $added}).name -eq 'Fixture ink'} 'Inline name did not rename the swatch'
    $first=(Panel).swatches[0]
    Invoke 'palette-name'
    $editor=Control 'palette-name-editor' -Type $ControlType::Edit
    $editor.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($first.name)
    $editor.SetFocus();[CapyRowPointer]::Key([uint16]0x0D)
    Wait-Until {(Control 'palette-message').Current.Name -ne ''} 'Duplicate name did not report an error'
    if(((Panel).swatches|Where-Object {$_.id -eq $added}).name -ne 'Fixture ink'){throw 'Duplicate name replaced the swatch name'}
    [CapyRowPointer]::Key([uint16]0x1B)
    Wait-Until {!(Find 'palette-name-editor')} 'Escape did not cancel the name editor'

    $tile=Control ('palette-swatch-'+(Swatches)[0])
    [CapyRowPointer]::RightClick((Center $tile).x,(Center $tile).y)
    Wait-Until {@(Menu-Items|Where-Object {$_.Current.AutomationId -eq 'palette-command-rename_color'}).Count} 'Swatch menu lacks Rename Color'
    Close-Menu
    $tile=Control ('palette-swatch-'+(Swatches)[1])
    [CapyRowPointer]::Down('touch',(Center $tile).x,(Center $tile).y);Start-Sleep -Milliseconds 900
    Wait-Until {@(Menu-Items|Where-Object {$_.Current.AutomationId -eq 'palette-command-rename_color'}).Count} 'Touch hold did not open the swatch menu' 4
    [CapyRowPointer]::Up();Close-Menu

    foreach($device in @('mouse','touch','pen')){
        $before=(Swatches) -join ','
        $a=Control ('palette-swatch-'+(Swatches)[0]);$b=Control ('palette-swatch-'+(Swatches)[2])
        Drag (Center $a) (Center $b) $device
        Wait-Until {((Swatches) -join ',') -ne $before} "$device drag did not reorder swatches"
        if(((Swatches)[2]) -ne ([uint64[]]($before -split ','))[0]){throw "$device drag placed the swatch in the wrong slot"}
        (Control ('palette-swatch-'+(Swatches)[0])).SetFocus()
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x5A)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $before} "Ctrl+Z did not undo the $device reorder"
    }
    $before=(Swatches) -join ','
    $a=Control ('palette-swatch-'+(Swatches)[0])
    Drag (Center $a) @{x=(Center $a).x-600;y=(Center $a).y}
    Start-Sleep -Milliseconds 300
    if(((Swatches) -join ',') -ne $before){throw 'Releasing outside the grid reordered swatches'}

    $active=(Panel).palette
    Invoke 'palette-selector'
    $search=Control 'palette-search' -Type $ControlType::Edit
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('pop')
    $target=((Panel).palettes|Where-Object {$_.name -eq 'Pop Art'}).id
    Wait-Until {(Find ('palette-choice-'+$target)) -and !(Find ('palette-choice-'+$active))} 'Search did not filter palettes'
    Capture 'chooser'
    Invoke ('palette-choice-'+$target)
    Wait-Until {(Panel).palette -eq $target -and !(Find 'palette-search')} 'Choosing a palette did not select it and close the chooser'

    $export=Join-Path $run 'fixture palette.gpl'
    Invoke 'palette-selector'
    $row=Control ('palette-choice-'+$target)
    [CapyRowPointer]::RightClick((Center $row).x,(Center $row).y)
    Wait-Until {@(Menu-Items|Where-Object {$_.Current.Name -eq 'Export Palette'}).Count} 'Palette menu lacks Export'
    $submenu=@(Menu-Items|Where-Object {$_.Current.Name -eq 'Export Palette'})[0]
    $submenu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Menu-Invoke 'palette-command-export_palette-gpl'
    Picker 'Save As' $export
    Wait-Until {(Test-Path -LiteralPath $export) -and (Model).windows_palettes.generation -ge 1 -and !(Model).windows_palettes.busy} 'GPL export did not finish' 20
    if((Model).windows_palettes.error){throw "Export failed: $((Model).windows_palettes.error)"}
    $palettes=@((Panel).palettes).Count
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    Invoke 'palette-library-add'
    Menu-Invoke 'palette-command-import_palette'
    Picker 'Open' $export
    Wait-Until {@((Panel).palettes).Count -eq $palettes+1 -and !(Model).windows_palettes.busy} 'GPL import did not add a palette' 20
    Capture 'imported'

    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $color=$null;foreach($zone in @((Model).header.model.zones)){foreach($entry in @($zone)){if($entry.item.control.kind -eq 'color'){$color=$entry.id}}}
    Tap (Center (Control ('header-item-'+$color)))
    Wait-Until {$d=(Model).state.customization.drawer;$d -and ((ConvertTo-Json -InputObject $d.columns -Compress) -eq '[["color","palettes"]]')} 'Sketch color drawer lacks Palettes below the wheel'
    Wait-Until {(Find 'drawer-panel-palettes') -and (Find 'palette-swatches' -Within (Find 'tool-drawer'))} 'Sketch drawer did not render Palettes'
    Capture 'sketch-drawer'
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Palettes review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Palettes review wrote to stderr'}
    Write-Output "Palettes acceptance passed: $run"
}catch{
    try{Capture 'failure'}catch{}
    try{@{panel=(Panel);files=(Model).windows_palettes}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    Set-Content -LiteralPath (Join-Path $run 'failure.txt') -Value ($_|Out-String)
    throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
