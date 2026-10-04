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
function MarkerPoint([string]$Id){
    $box=(Control $Id -Arranged).Current.BoundingRectangle;$inset=6*[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
    @{x=[int]($box.Right-$inset);y=[int]($box.Bottom-$inset)}
}
function TapMarker([string]$Id,[string]$Device){
    $at=MarkerPoint $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
}
function Context([string]$Id){$at=MarkerPoint $Id;[CapyRowPointer]::RightClick($at.x,$at.y)}
function Menus{
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem))
    @([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {!$_.Current.IsOffscreen})
}
function Choose([string]$Id,[scriptblock]$View,[string]$Device){
    $before=& $View
    TapMarker $Id $Device
    Wait-Until {(& $View).selected} 'Marker click did not activate the tool'
    if(@(Menus).Count){throw 'Primary marker click opened a context menu'}
    if(!$before.selected){TapMarker $Id $Device}
    Wait-Until {$anchor=(Model).state.customization.drawer.anchor;if($Id.StartsWith('tile-')){$anchor.tile -eq $before.id}else{$anchor.id -eq $before.id}} 'Active marker click did not open the tool drawer'
    if(@(Menus).Count){throw 'Active marker click opened a context menu'}
    TapMarker $Id $Device
    Wait-Until {!(Model).state.customization.drawer} 'Marker click did not toggle the tool drawer closed'
    $layout=(Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress
    Context $Id
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
    if((Control $Id).Current.Name -ne (& $View).label){throw 'Tool retained its earlier accessibility label'}
    Context $Id
    Wait-Until {@(Menus).Count -ge 2} 'Tool variations did not reopen'
    $selected=@(Menus|Where-Object {$_.Current.Name -eq $name})[0]
    if($selected.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::On){throw 'Remembered variation is not checked'}
    $selected.SetFocus();[CapyRowPointer]::Key(0x1b)
    Wait-Until {@(Menus).Count -eq 0} 'Escape did not dismiss the variation menu'
    @{device=$Device;button=$Id;label=$name;options=$names}
}
function Hidden-Zone([int]$Id){
    try{$presentation=(Control 'title-bar').Current.ItemStatus|ConvertFrom-Json}catch{return $null}
    $zones=@($presentation.geometry.hidden)
    for($zone=0;$zone -lt $zones.Count;$zone++){if(@($zones[$zone]) -contains $Id){return $zone}}
    $null
}
function Overflow-Variation([int]$Id,[int]$Zone,[string]$Device){
    $view={@((Model).header.items|Where-Object id -eq $Id)[0]};$before=& $view
    Invoke "header-overflow-$Zone"
    $row=Control "header-overflow-item-$Id" -Arranged
    $image=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Image)
    if(!$row.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$image)){throw 'Overflowed grouped tool lost its marker'}
    if($Device -eq 'mouse'){Capture 'header-overflow-list' -Composed}
    $box=$row.Current.BoundingRectangle;$x=[int]($box.X+$box.Width/2);$y=[int]($box.Y+$box.Height/2)
    if($Device -eq 'mouse'){[CapyRowPointer]::RightClick($x,$y)}else{[CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 1200;[CapyRowPointer]::Up()}
    Wait-Until {@(Menus).Count -ge 2} "$Device did not open variations from the title-bar overflow"
    if(Find "header-overflow-item-$Id" -Visible){throw 'Overflow list stayed open under the variation menu'}
    $choice=$null
    foreach($option in @(Menus)){
        $toggle=$null
        if($option.Current.IsEnabled -and $option.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$toggle) -and $toggle.Current.ToggleState -eq [System.Windows.Automation.ToggleState]::Off){$choice=$option;break}
    }
    if(!$choice){throw 'Overflow variation menu has no alternate enabled choice'}
    $name=$choice.Current.Name;$choice.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {$current=& $view;($current.label -eq $name -or $current.label.StartsWith($name+' ')) -and $current.icon -ne $before.icon} 'Overflow variation did not reach the hidden tool'
    Wait-Until {@(Menus).Count -eq 0} 'Overflow variation menu did not close'
    @{device=$Device;overflow=$Zone;label=$name}
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
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
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
        foreach($device in @('mouse','pen','touch')){$results+=Choose "tile-toolbar-$($slot.id)" {Tile $slot.id} $device}
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
        if(!$eraser -or !$eraser.has_variants){throw 'Eraser category has no preset choices'}
        if(@(@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.kind -eq 'brush' -and $_.has_variants}).Count){throw 'Pinned brush preset has category choices'}
        Tap "tile-toolbar-$($eraser.id)"
        Wait-Until {(Tile $eraser.id).selected} 'Eraser category did not activate'
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
    foreach($device in @('mouse','pen','touch')){$results+=Choose "header-item-$($entry.id)" {@((Model).header.items|Where-Object id -eq $entry.id)[0]} $device}
    Capture 'header-variations' -WithModel -Composed
    $zone=$null
    foreach($width in 1100,1000,900,800,700,600,520){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width $width -Height 760
        try{Wait-Until {$null -ne ($script:hiddenZone=Hidden-Zone $entry.id)} 'Not hidden' 3;$zone=$script:hiddenZone;break}catch{}
    }
    if($null -eq $zone){throw 'The grouped title-bar tool never overflowed'}
    Capture 'header-overflow' -WithModel -Composed
    foreach($device in @('mouse','pen','touch')){$results+=Overflow-Variation $entry.id $zone $device}
    $results|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run 'results.json')
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    [pscustomobject]@{theme=$Theme;toolbar='passed';header='passed';header_overflow='passed';devices=@('mouse','pen','touch');eraser_group='passed';checked_choices='passed';layout_unchanged='passed';evidence=$run}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{Exit-CapyEnvironment}
