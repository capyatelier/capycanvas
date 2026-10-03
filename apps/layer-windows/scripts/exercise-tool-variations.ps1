param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$CapyPopups=$true;$CapyFind='prefer-visible'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/tool-variations/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
function Tile([int]$Id){@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object id -eq $Id}
function Point([string]$Id){$box=(Control $Id -Arranged).Current.BoundingRectangle;@{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}}
function Tap([string]$Id,[string]$Device='mouse'){
    $at=Point $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
}
function Menus{
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    @([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {!$_.Current.IsOffscreen})
}
function Choose([string]$Id,[scriptblock]$View,[string]$Device){
    $before=& $View;$layout=(Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress
    Tap $Id $Device
    Wait-Until {@(Menus).Count -ge 2} 'Tool variations did not open'
    $options=@(Menus);$names=@($options|ForEach-Object {$_.Current.Name});$choice=$null
    foreach($option in $options){
        if(!$option.Current.IsEnabled){continue}
        $toggle=$null
        if($option.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$toggle) -and $toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$choice=$option;break}
    }
    if(!$choice){throw 'Variation menu has no alternate enabled choice'}
    $name=$choice.Current.Name
    $choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {$current=& $View;($current.label -eq $name -or $current.label.StartsWith($name+' ')) -and $current.selected -and $current.icon -ne $before.icon} 'Chosen variation did not reach the projected tool'
    Wait-Until {@(Menus).Count -eq 0} 'Variation menu did not close'
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress) -ne $layout){throw 'Choosing a variation changed workspace layout'}
    if((Control $Id).Current.Name -ne (& $View).label){throw 'Variation button retained its earlier accessibility label'}
    Tap $Id $Device
    Wait-Until {@(Menus).Count -ge 2} 'Tool variations did not reopen'
    $selected=@(Menus|Where-Object {$_.Current.Name -eq $name})[0]
    if($selected.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::On){throw 'Remembered variation is not checked'}
    $selected.SetFocus();[CapyRowPointer]::Key(0x1b)
    Wait-Until {@(Menus).Count -eq 0} 'Escape did not dismiss the variation menu'
    @{device=$Device;button=$Id;label=$name;options=$names}
}
function Workspace([string]$Id){
    $choice=@((Model).windows_workspace.switcher|Where-Object id -eq $Id)[0]
    if((Model).windows_workspace.id -eq $Id){return}
    $toggle=Find ('workspace-switch-'+$Id) -Visible
    if(!$toggle){Invoke 'header-workspace-menu';$toggle=Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $toggle.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} 'Workspace did not open'
}
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json'),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Review did not start' 120
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $results=@()
    foreach($workspace in @('builtin:workspace:illustrator','builtin:workspace:photographer')){
        Workspace $workspace
        $slots=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object has_variants
        if(!@($slots).Count){throw 'Built-in workspace has no grouped tools'}
        $slot=@($slots|Where-Object {$_.control.slot -in @('manual_selection','marquee')})[0]
        if(!$slot){throw 'Workspace has no grouped selection tool'}
        foreach($device in @('mouse','pen','touch')){$results+=Choose "tile-variants-toolbar-$($slot.id)" {Tile $slot.id} $device}
        Tap "tile-toolbar-$($slot.id)"
        Wait-Until {(Model).state.customization.drawer.anchor.tile -eq $slot.id -and (Model).state.customization.drawer.tool_set.groups.Count -ge 2} 'Grouped tool did not open its scoped drawer'
        $drawer=Control 'tool-drawer' -Arranged
        $groups=@((Model).state.customization.drawer.tool_set.groups)
        for($i=0;$i -lt $groups.Count;$i++){
            $button=Control "tool-group-$i" -Within $drawer -Arranged
            if($button.Current.Name -ne $groups[$i].label -or $button.Current.IsEnabled -ne $groups[$i].enabled){throw 'Drawer did not present the shared sibling choices'}
        }
        Tap "tile-toolbar-$($slot.id)"
        Wait-Until {!(Model).state.customization.drawer} 'Grouped drawer did not toggle closed'
        $eraser=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.command -eq 'eraser'}|Select-Object -First 1
        if(!$eraser -or $eraser.has_variants){throw 'Eraser is not an independent pinned tool'}
        Tap "tile-toolbar-$($eraser.id)"
        Wait-Until {(Tile $eraser.id).selected} 'Pinned Eraser did not activate'
        Capture ($workspace.Split(':')[-1]) -WithModel -Composed
    }
    Wait-Until {@(Menus).Count -eq 0} 'Earlier menu remained visible before header customization'
    (Control 'drawing-canvas').SetFocus();Start-Sleep -Milliseconds 350
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    Invoke 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Header customization did not open'
    $from=Point 'header-component-tools'
    $presentation=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json
    $zone=$presentation.geometry.zones[1];$origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Header client origin unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
    [CapyRowPointer]::Down('mouse',$from.x,$from.y)
    [CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width*.5)*$scale),[int]($origin.y+($zone.y+$zone.height*.5)*$scale))
    Wait-Until {try{((Control 'title-bar').Current.HelpText|ConvertFrom-Json).preview.target}catch{$false}} 'Header tool insertion has no target'
    [CapyRowPointer]::Up();Wait-Until {(Model).picker} 'Header tool picker did not open'
    $choice=@((Model).picker.choices|Where-Object {$_.control.kind -eq 'tool_slot' -and $_.control.slot -eq 'drawing'})[0]
    if(!$choice){throw 'Grouped drawing tool is missing from customization'}
    (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($choice.label)
    Wait-Until {(Model).picker.query -eq $choice.label} 'Tool search did not reach shared state'
    (Control 'picker-choice-tool_slot-drawing').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Invoke 'Add Tools' -Name
    Wait-Until {!(Model).picker} 'Tool picker did not close'
    $entry=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.kind -eq 'tool_slot' -and $_.item.control.slot -eq 'drawing'})[0]
    if(!$entry){throw 'Header tool slot was not inserted'}
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Header customization did not finish'
    foreach($device in @('mouse','pen','touch')){$results+=Choose "header-variants-$($entry.id)" {@((Model).header.items|Where-Object id -eq $entry.id)[0]} $device}
    Capture 'header-variations' -WithModel -Composed
    $results|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run 'results.json')
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    [pscustomobject]@{theme=$Theme;toolbar='passed';header='passed';devices=@('mouse','pen','touch');pinned_eraser='passed';checked_choices='passed';layout_unchanged='passed';evidence=$run}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{Exit-CapyEnvironment}
