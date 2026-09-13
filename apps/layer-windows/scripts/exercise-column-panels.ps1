param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class CapyColumnDpi { [StructLayout(LayoutKind.Sequential)] public struct Point {public int X,Y;} [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window); [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point); }'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/column-panels/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Read-Snapshot([string]$Path){
    $stream=[IO.File]::Open($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.StreamReader]::new($stream)
    try{$reader.ReadToEnd()}finally{$reader.Dispose()}
}
function Model {
    try {
        if(!$script:statePath){
            # Per-window snapshots use atomic replacement. The compatibility
            # ui-state.json path is a direct write and can be read mid-frame.
            foreach($candidate in [IO.Directory]::EnumerateFiles($directory,("ui-state-"+$review.Id+"-*.json"))){
                if([IO.File]::GetLastWriteTimeUtc($candidate) -lt $review.StartTime.ToUniversalTime()){continue}
                $initial=Read-Snapshot $candidate|ConvertFrom-Json
                if($initial.process_id -eq $review.Id -and $initial.model.windows_isolated_settings){
                    $script:statePath=$candidate;$script:windowId=$initial.window_id;break
                }
            }
        }
        if(!$script:statePath){return}
        $value=Read-Snapshot $script:statePath|ConvertFrom-Json
        if($value.process_id -eq $review.Id -and $value.window_id -eq $script:windowId -and $value.model.windows_isolated_settings){$value.model}
    }catch{}
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        [CapyRowPointer]::Verify()
        if(& $Predicate){return}
        $review.Refresh();if($review.HasExited){throw 'Owned column review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $candidates=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)
    $found=$null
    foreach($candidate in $candidates){
        if(!$candidate.Current.IsOffscreen){$found=$candidate;break}
    }
    if(!$found -and $candidates.Count){$found=$candidates[0]}
    if(!$found -and $Within -eq $root){
        # A cascaded WinUI menu can live in a separate UIA fragment.
        # Search only this fixture's owned process, including its popup HWNDs.
        $owned=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
        $found=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$owned)
    }
    $found
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null}
    Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    $item=Control $Value -Name:$Name -Within $Within
    $pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    elseif($item.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}
    else{throw "Control $Value does not support activation"}
}
function Point([string]$Id,[switch]$Name) {
    $stable=@{bounds=$null;count=0}
    Wait-Until {
        $item=Find $Id -Name:$Name
        if(!$item -or $item.Current.IsOffscreen){return $false}
        $bounds=$item.Current.BoundingRectangle
        if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){return $false}
        if($bounds -eq $stable.bounds){$stable.count++}else{$stable.bounds=$bounds;$stable.count=0}
        $stable.count -ge 2
    } "Source $Id did not arrange visibly"
    # A row-root source uses its transparent left padding, outside child buttons.
    $x=if($Id -like 'layer-row-*'){$stable.bounds.X+2}else{$stable.bounds.X+$stable.bounds.Width*.5}
    @{x=[int]$x;y=[int]($stable.bounds.Y+$stable.bounds.Height*.5)}
}

function Gesture {try{(Find 'Drawing workspace' -Name).Current.HelpText|ConvertFrom-Json}catch{}}
function Presentation {try{(Find 'Drawing workspace' -Name).Current.ItemStatus|ConvertFrom-Json}catch{}}
function Layout {(Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress}
function Panel-Group([string]$Panel) {(Model).layout.groups|Where-Object {$_.panels -contains $Panel}}
function Workspace-History([string]$Id) {
    # Local contact release precedes Rust completion and native publication.
    # Open the menu only after its command state has reached the same view.
    Wait-Until {
        $m=Model;$native=Presentation
        @($m.state.commands|Where-Object {$_.id -eq $Id -and $_.enabled}).Count -eq 1 -and
            $native.revision -ge $m.workspace_update.revision -and !$m.windows_workspace.busy
    } "History action $Id did not finish publishing"
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    Invoke $Id
}
function Tap([string]$Id) {
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Up()
}
function Cancel-Contact {
    if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}
    else{[CapyRowPointer]::Cancel()}
}
function Client-Origin {
    $point=[CapyColumnDpi+Point]::new()
    if(![CapyColumnDpi]::ClientToScreen($review.MainWindowHandle,[ref]$point)){throw 'Cannot measure client origin'}
    $point
}
function Canvas-Point {
    $area=(Model).layout.work_area
    $bounds=Client-Origin
    $scale=[CapyColumnDpi]::GetDpiForWindow($review.MainWindowHandle)/96.0
    @{x=[int]($bounds.X+($area.x+$area.width*.5)*$scale);y=[int]($bounds.Y+($area.y+$area.height*.5)*$scale)}
}
function Set-Theme([string]$Theme) {
    Invoke 'Preferences' -Name
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    $picker=Control 'Color theme' -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::ComboBox)
    $picker.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Theme -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq $Theme.ToLowerInvariant()} 'Theme did not reach shared state'
    Invoke 'Close' -Name -Within $dialog
    Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
}
function Context-Action([string]$Id,[string]$Action) {
    Start-Sleep -Milliseconds 300
    $at=Point $Id
    [CapyRowPointer]::RightClick($at.x,$at.y)
    Invoke $Action -Name
    Start-Sleep -Milliseconds 300
}
function Column([int]$Id) { @((Model).layout.collapsed|Where-Object id -eq $Id)[0] }
function Assert-Stack([int]$Id,[string]$Contains='') {
    $stable=@{}
    Wait-Until {
        $column=Column $Id;$panel=$column.group_panel
        if(!$panel -or ($Contains -and $panel.panels.panel -notcontains $Contains)){return $false}
        $drawer=Find "column-drawer-$Id"
        if(!$drawer -or !(Find "group-panel-resize-$Id-width")){return $false}
        try{$geometry=$drawer.Current.ItemStatus|ConvertFrom-Json}catch{return $false}
        if($geometry.placement.columns.Count -ne $panel.panels.Count){return $false}
        foreach($key in @('x','y','width','height')){
            if([Math]::Abs($geometry.placement.bounds.$key-$panel.bounds.$key) -gt .02){return $false}
        }
        $stable.column=$column;$stable.panel=$panel;$true
    } 'Attached group panel did not publish matching native geometry'
    $column=$stable.column;$panel=$stable.panel
    $origin=Client-Origin
    $scale=[CapyColumnDpi]::GetDpiForWindow($review.MainWindowHandle)/96.0
    for($index=0;$index -lt $panel.panels.Count;$index++){
        $member=$panel.panels[$index];$null=Control "drawer-panel-$($member.panel)"
        Wait-Until {
            $actual=(Control "drawer-slot-$Id-$index").Current.BoundingRectangle
            [Math]::Abs($actual.X-($origin.X+$member.bounds.x*$scale)) -le 1 -and
            [Math]::Abs($actual.Y-($origin.Y+$member.bounds.y*$scale)) -le 1 -and
            [Math]::Abs($actual.Width-$member.bounds.width*$scale) -le 1 -and
            [Math]::Abs($actual.Height-$member.bounds.height*$scale) -le 1
        } "Native slot $Id/$index does not match shared physical bounds"
    }
    if($null -ne (Find "drawer-grip-$($panel.group)")){throw 'Attached group incorrectly kept the drawer tab bar'}
    if([Math]::Abs($panel.bounds.height-$column.bounds.height) -gt .02){throw 'Group panel is not full height'}
    Write-Output "Attached column ${Id}: $($panel.panels.Count) panels"
}
function Resize-Panel([int]$Id,[string]$Handle,[string]$Axis,[switch]$Cancel) {
    $script:case="resize-$Id-$Handle-$(if($Cancel){'cancel'}else{'commit'})"
    $before=Layout
    $members=(Column $Id).group_panel.panels
    $runtime=@{}
    foreach($member in $members){$runtime[$member.panel]=(Control "drawer-panel-$($member.panel)").GetRuntimeId() -join ','}
    $at=Point $Handle
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 35
    $x=$at.x;$y=$at.y
    if($Axis -eq 'x'){$x+=45}else{$y+=45}
    [CapyRowPointer]::Move($x,$y)
    Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).captured -and (Layout) -ne $before} "$case did not resize with capture"
    $gesture=Gesture
    if($gesture.requires_hold -or $gesture.device -ne $Device -or $gesture.menu_open){throw "$case used incorrect pickup arbitration"}
    foreach($member in $members){
        if(((Control "drawer-panel-$($member.panel)").GetRuntimeId() -join ',') -ne $runtime[$member.panel]){throw "$case rebuilt $($member.panel)"}
    }
    if($Cancel){
        Cancel-Contact
        Wait-Until {(Gesture).phase -eq 'idle' -and (Layout) -eq $before} "$case failed to roll back"
    }else{
        [CapyRowPointer]::Up()
        Wait-Until {(Gesture).phase -eq 'idle'} "$case did not finish"
        $after=Layout
        Workspace-History 'undo_workspace';Wait-Until {(Layout) -eq $before} "$case Undo was not one step"
        Workspace-History 'redo_workspace';Wait-Until {(Layout) -eq $after} "$case Redo was not one step"
    }
    Assert-Stack $Id
    Write-Output "$case passed"
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $run 'stdout.log') -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/column-panels-review.json')
    Write-Output "Owned column panel review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Column review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $script:case='open-left-stack'
    Context-Action 'panel-tab-sizes' 'Collapse column'
    $left=@((Model).layout.collapsed|Where-Object {($_.groups.icons.panel) -contains 'sizes'})[0].id
    Context-Action 'column-icon-sizes' 'Group panel'
    Tap 'column-icon-sizes'
    Assert-Stack $left
    $first=(Column $left).group_panel.panels[0].panel
    Resize-Panel $left "group-panel-resize-$left-width" 'x' -Cancel
    Resize-Panel $left "group-panel-resize-$left-width" 'x'
    Resize-Panel $left "group-panel-resize-$left-$first" 'y' -Cancel
    Resize-Panel $left "group-panel-resize-$left-$first" 'y'
    $saved=Layout
    Tap 'column-icon-sizes'
    Wait-Until {$null -eq (Column $left).group_panel -and $null -eq (Find "column-drawer-$left")} 'Same group icon did not close attached panel'
    Tap 'column-icon-sizes';Assert-Stack $left
    if((Layout) -ne $saved){throw 'Reopening changed saved sizes'}
    $script:case='auto-hide'
    Context-Action 'column-icon-sizes' 'Auto-hide'
    $beforeDocument=(Model).state.document_file.modified
    $at=Canvas-Point
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 35;[CapyRowPointer]::Up()
    Wait-Until {$null -eq (Column $left).group_panel} 'Outside contact did not auto-hide'
    if((Model).state.document_file.modified -ne $beforeDocument){throw 'Dismissal painted the canvas'}
    Tap 'column-icon-sizes';Assert-Stack $left
    Context-Action 'column-icon-sizes' 'Drawers'
    Wait-Until {$null -eq (Column $left).group_panel -and $null -ne (Find 'drawer-tab-sizes')} 'Switching back to drawers did not retain the open group'
    Context-Action 'column-icon-sizes' 'Group panel';Assert-Stack $left
    Context-Action 'column-icon-sizes' 'Auto-hide'
    $script:case='apply-all-and-right-stack'
    Context-Action 'column-icon-sizes' 'Apply to all columns'
    Context-Action 'panel-tab-properties' 'Collapse column'
    $right=@((Model).layout.collapsed|Where-Object {($_.groups.icons.panel) -contains 'properties'})[0].id
    Tap 'column-icon-properties';Assert-Stack $right 'properties'
    Resize-Panel $right "group-panel-resize-$right-width" 'x' -Cancel
    Resize-Panel $right "group-panel-resize-$right-width" 'x'
    $script:case='navigator-and-layers'
    Tap 'column-icon-navigator';Assert-Stack $right 'navigator'
    $null=Control 'navigator-overview'
    Invoke 'navigator-zoom_in'
    Tap 'column-icon-layers';Assert-Stack $right 'layers'
    $null=Control 'layer-row-1'
    $script:case='zen-columns'
    Invoke 'Zen mode' -Name
    Wait-Until {(Model).chrome_hidden -and !(Find "column-drawer-$left") -and !(Find "column-drawer-$right")} 'Zen left column panels visible'
    (Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key(0x09)
    Wait-Until {!(Model).chrome_hidden} 'Leaving Zen did not restore chrome'
    Assert-Stack $left;Assert-Stack $right 'layers'
    foreach($theme in @('Dark','Light')){
        $script:case="theme-$theme"
        Set-Theme $theme
        if(!(Column $left).group_panel){Tap 'column-icon-sizes'}
        if(!(Column $right).group_panel){Tap 'column-icon-layers'}
        Assert-Stack $left;Assert-Stack $right
        $settled=@{key=$null;count=0}
        Wait-Until {
            $m=Model;$key=@($m.layout,$m.state.camera,$m.state.theme)|ConvertTo-Json -Depth 60 -Compress
            if($key -eq $settled.key){$settled.count++}else{$settled.key=$key;$settled.count=0}
            $settled.count -ge 4
        } 'Theme geometry did not settle before capture'
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ("attached-$theme.png")) -ClientOnly *> (Join-Path $run ("attached-$theme.json"))
    }
    $persisted=(Model).state.workspace.layout.column_settings|ConvertTo-Json -Depth 20 -Compress
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    $script:case='restart-settings';$script:statePath=$null;$script:windowId=$null
    $stderr=Join-Path $run 'restart-stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/column-panels-review.json')
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Restart did not adopt saved workspace' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if(((Model).state.workspace.layout.column_settings|ConvertTo-Json -Depth 20 -Compress) -ne $persisted){throw 'Column preferences or sizes did not survive restart'}
    if(@((Model).layout.collapsed|Where-Object group_panel).Count){throw 'Restart persisted transient open groups'}
    Tap 'column-icon-sizes';Assert-Stack $left
    Tap 'column-icon-layers';Assert-Stack $right 'layers'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Restart stderr requires inspection'}
    [pscustomobject]@{device=$Device;stacked_projection='passed';retained_resize='passed';resize_cancel='passed';one_step_history='passed';reopen_sizes='passed';auto_hide='passed';mode_switch='passed';apply_all='passed';navigator_and_layers='passed';both_themes='passed';zen_columns='passed';restart_preferences='passed';zero_exit='passed';scope='native controls and OS-injected input; physical digitizers and latency remain separate'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'result.json')
}catch{
    $failure=$_
    @{case=$script:case;error=$failure.ToString();gesture=(Gesture);presentation=(Presentation)}|ConvertTo-Json -Depth 60|Set-Content (Join-Path $run 'failure.json')
    if($review -and !$review.HasExited){
        & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure-capture.json')
    }
    throw $failure
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){
        if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
        else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
    }
}
