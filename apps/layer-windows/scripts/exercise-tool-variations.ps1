param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','sketch','sketch-light')][string]$Journey='full')
$ErrorActionPreference='Stop'
if($Journey -eq 'sketch-light'){$Theme='light'}
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
    $box=(Control $Id -Arranged).Current.BoundingRectangle;$inset=6*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
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
    @([System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {!$_.Current.IsOffscreen -and $_.Current.AutomationId -notlike 'application-menu*'})
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
function Tile-Images([int]$Id){
    $bounds=(Control "tile-toolbar-$Id" -Arranged).Current.BoundingRectangle
    $image=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Image)
    $tileImages=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$image)|Where-Object {$box=$_.Current.BoundingRectangle;!$_.Current.IsOffscreen -and !$box.IsEmpty -and $bounds.Contains($box)})
    $tileImages
}
function Leaf-Eraser([int]$Id,[string]$Device){
    $before=Tile $Id;$file=(Model).state.document_file|ConvertTo-Json -Depth 8 -Compress
    if($before.has_variants){throw 'Single-preset Eraser retained category choices'}
    $images=@(Tile-Images $Id)
    if($images.Count -ne 1){throw 'Single-preset Eraser has a visible group marker or lost its icon'}
    Tap "tile-toolbar-$Id" $Device
    Wait-Until {(Tile $Id).selected} 'Leaf Eraser did not activate'
    if(!$before.selected){
        if((Model).state.customization.drawer){throw 'First Eraser activation opened settings'}
        Tap "tile-toolbar-$Id" $Device
    }
    Wait-Until {$drawer=(Model).state.customization.drawer;$drawer.anchor.tile -eq $Id -and !$drawer.tool_set -and @($drawer.columns).Count -eq 1 -and @($drawer.columns[0]).Count -eq 1 -and $drawer.columns[0][0] -eq 'tool_settings'} 'Leaf Eraser did not open only its settings at the native origin'
    $drawer=Control 'tool-drawer' -Arranged
    $null=Control 'tool-setting-size' -Within $drawer -Arranged
    if((Find 'tool-group-0' -Within $drawer -Visible) -or (Find 'tool-subtool-0' -Within $drawer -Visible)){throw 'Leaf Eraser retained a redundant native chooser'}
    Capture ($workspace.Split(':')[-1]+'-eraser-'+$Device) -WithModel -Composed
    Tap "tile-toolbar-$Id" $Device
    Wait-Until {!(Model).state.customization.drawer -and !(Find 'tool-drawer' -Visible)} 'Leaf Eraser did not toggle its native settings closed'
    if(((Model).state.document_file|ConvertTo-Json -Depth 8 -Compress) -ne $file){throw 'Leaf Eraser input changed artwork or history'}
    @{device=$Device;button="tile-toolbar-$Id";leaf_eraser='passed';native_marker='absent';settings_only='passed';artwork_unchanged='passed'}
}
function Merged-Presets {
    foreach($set in @(@{command='pen';count=10;name='Marker';ids=@(1,44,28,33,29,30,31,32,34,7)},@{command='airbrush';count=2;name='Spray';ids=@(5,8)})){
        $tile=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.command -eq $set.command}|Select-Object -First 1
        if(!$tile -or !$tile.has_variants){throw "Missing grouped $($set.command) tool"}
        Tap "tile-toolbar-$($tile.id)"
        Wait-Until {(Tile $tile.id).selected} "The $($set.command) tool did not activate"
        $presets=@((Model).state.tool_set.subtools|Where-Object {$_.action.type -eq 'select_brush' -or ($_.action.type -eq 'choose_tool_variant' -and $_.action.variant.type -eq 'brush_preset')})
        if($presets.Count -ne $set.count){throw "The $($set.command) tool did not project its complete preset set"}
        $ids=@($presets|ForEach-Object {
            if($_.action.type -eq 'select_brush'){$_.action.id}else{
                $anchor=$_.action.anchor
                if($anchor.kind -ne 'tile' -or $anchor.panel -ne 'toolbar' -or $anchor.tile -ne $tile.id){throw 'A merged preset lost its native toolbar origin'}
                $_.action.variant.id
            }
        })
        if((($ids|Sort-Object)-join ',') -ne (($set.ids|Sort-Object)-join ',') -or @($presets|Where-Object {$expectedIcon=if($_.label -eq 'Marker'){'marker'}elseif($_.label -eq 'Spray'){'spray'}else{$set.command};$_.icon -ne $expectedIcon -or !$_.label}).Count -or @($presets.label|Select-Object -Unique).Count -ne $set.count){throw 'A merged preset lost its stable ID, name or preset icon'}
        Context "tile-toolbar-$($tile.id)"
        Wait-Until {@(Menus|Where-Object {$_.Current.Name -in $presets.label}).Count -eq $set.count} "The $($set.command) menu did not expose its complete preset set"
        if(@(Menus|Where-Object {$_.Current.Name -eq $set.name -and $_.Current.IsEnabled}).Count -ne 1){throw "$($set.name) is not a discoverable preset of $($set.command)"}
        @(Menus)[0].SetFocus();[CapyRowPointer]::Key(0x1b)
        Wait-Until {@(Menus).Count -eq 0} 'Merged preset menu did not dismiss'
        @{command=$set.command;presets=$set.count;discoverable=$set.name}
    }
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
function Sketch-Drawers {
    Workspace 'builtin:workspace:painter'
    Maximize-DrawingWindow
    Wait-Until {(Model).canvas_ready -and (Model).brush_ready} 'Sketch canvas did not become ready' 120
    $paperPoint=Clear-CanvasPoint 'title-bar' 40 0;$park=Point 'title-bar'
    [CapyRowPointer]::Hover($park.x,$park.y)
    $blank=Wait-StablePixels {Screen-Pixels $paperPoint[0] $paperPoint[1]}
    $revision=(Model).state.document_file.revision
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Down('pen',$paperPoint[0],$paperPoint[1])
    try{foreach($dx in 5,10,15,20,25,30,35,40){[CapyRowPointer]::Move(($paperPoint[0]+$dx),$paperPoint[1]);Start-Sleep -Milliseconds 12};[CapyRowPointer]::Up()}finally{[CapyRowPointer]::Cancel()}
    [CapyRowPointer]::Hover($park.x,$park.y)
    Wait-Until {(Model).state.document_file.revision -gt $revision -and (Screen-Pixels $paperPoint[0] $paperPoint[1]) -ne $blank} 'The native Sketch pen did not paint actual pixels' 45
    $painted=Wait-StablePixels {Screen-Pixels $paperPoint[0] $paperPoint[1]}
    $file=(Model).state.document_file|ConvertTo-Json -Depth 8 -Compress
    $entries=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'tool'})
    $keys=@($entries|ForEach-Object {$c=$_.item.control;$c.kind+':'+$c.command+$c.panel})
    if((($keys|Sort-Object) -join ',') -ne ((@('panel:adjustments','command:select','command:scale_rotate','command:drawing_brush','command:sculpt','command:eraser','panel:layers','color:')|Sort-Object) -join ',')){throw 'Sketch did not retain every default header tool'}
    $records=@()
    foreach($entry in $entries){
        $id=$entry.id;$spec=@((Model).header.items|Where-Object id -eq $id)[0]
        $source=Control "header-item-$id" -Arranged
        if(!$spec.enabled -or !$source.Current.IsEnabled -or $source.Current.Name -ne $spec.label){throw "Sketch header tool is unavailable or stale: $($spec.label)"}
        Tap "header-item-$id"
        Wait-Until {(Model).state.customization.drawer.anchor.id -eq $id -or @((Model).header.items|Where-Object id -eq $id)[0].selected} "Sketch did not activate $($spec.label)" 45
        if((Model).state.customization.drawer.anchor.id -ne $id){Tap "header-item-$id"}
        Wait-Until {(Model).state.customization.drawer.anchor.id -eq $id} "Sketch did not open $($spec.label) at its header origin" 45
        $drawer=Control 'tool-drawer' -Arranged
        foreach($column in (Model).state.customization.drawer.columns){foreach($panel in $column){$null=Control ('drawer-panel-'+$panel) -Within $drawer -Arranged}}
        $native=$drawer.Current.BoundingRectangle
        if(!$drawer.Current.IsEnabled -or !$root.Current.BoundingRectangle.Contains($native)){throw "Sketch drawer is disabled or outside its window: $($spec.label)"}
        $geometry=$drawer.Current.ItemStatus|ConvertFrom-Json;$anchor=$geometry.placement.anchor
        $bar=(Control 'title-bar').Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        $actual=(Control "header-item-$id" -Arranged).Current.BoundingRectangle
        foreach($pair in @(@($actual.X,($bar.X+$anchor.x*$scale)),@($actual.Y,($bar.Y+$anchor.y*$scale)),@($actual.Width,($anchor.width*$scale)),@($actual.Height,($anchor.height*$scale)))){
            if([Math]::Abs($pair[0]-$pair[1]) -gt 1){throw "Sketch drawer anchor differs from its native header tool: $($spec.label)"}
        }
        Capture ('sketch-drawer-'+$id) -WithModel -Composed
        $records+=@{id=$id;label=$spec.label;columns=(Model).state.customization.drawer.columns;native_anchor=@($actual.X,$actual.Y,$actual.Width,$actual.Height);shared_anchor=$anchor;native_drawer=@($native.X,$native.Y,$native.Width,$native.Height)}
        Tap "header-item-$id"
        Wait-Until {!(Model).state.customization.drawer -and !(Find 'tool-drawer' -Visible)} "Sketch did not close $($spec.label)"
        (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Hover($park.x,$park.y)
        if(((Model).state.document_file|ConvertTo-Json -Depth 8 -Compress) -ne $file){throw "Sketch drawer changed artwork or history: $($spec.label)"}
        Wait-Until {(Screen-Pixels $paperPoint[0] $paperPoint[1]) -eq $painted} "Sketch drawer changed composed artwork pixels: $($spec.label)" 45
    }
    [IO.File]::WriteAllText((Join-Path $run 'sketch-drawers.json'),($records|ConvertTo-Json -Depth 12),[Text.UTF8Encoding]::new($false))
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
    Wait-Until {(Screen-Pixels $paperPoint[0] $paperPoint[1]) -eq $blank} 'Sketch Undo did not restore exact blank pixels' 45
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),0x5a)
    Wait-Until {(Screen-Pixels $paperPoint[0] $paperPoint[1]) -eq $painted} 'Sketch Redo did not restore exact painted pixels' 45
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),0x5a)
    Wait-Until {(Screen-Pixels $paperPoint[0] $paperPoint[1]) -eq $blank} 'Sketch did not return to its exact blank artwork' 45
    @{theme=$Theme;default_sketch_drawers=$records.Count;native_pen_pixels='passed';undo_redo_pixels='exact';drawer_artwork_unchanged='passed'}
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
    $null=$review.Handle
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Review did not start' 120
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $results=@()
    if($Journey -ne 'full'){
        $results=@(Sketch-Drawers)
        [IO.File]::WriteAllText((Join-Path $run 'results.json'),($results|ConvertTo-Json -Depth 6),[Text.UTF8Encoding]::new($false))
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        $results|ConvertTo-Json -Depth 6
        return
    }
    foreach($workspace in @('builtin:workspace:illustrator','builtin:workspace:photographer')){
        Workspace $workspace
        $slots=@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object has_variants
        if(!@($slots).Count){throw 'Built-in workspace has no grouped tools'}
        $slot=@($slots|Where-Object {$_.control.slot -in @('manual_selection','marquee')})[0]
        if(!$slot){throw 'Workspace has no grouped selection tool'}
        if(@(Tile-Images $slot.id).Count -ne 2){throw 'Grouped selection tool lost its native icon or marker'}
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
        if(!$eraser){throw 'The native toolbar has no Eraser'}
        if(@(@((Model).panels|Where-Object id -eq 'toolbar')[0].tiles|Where-Object {$_.control.kind -eq 'brush' -and $_.has_variants}).Count){throw 'Pinned brush preset has category choices'}
        foreach($device in @('mouse','pen','touch')){$results+=Leaf-Eraser $eraser.id $device}
        if($workspace -eq 'builtin:workspace:illustrator'){$results+=Merged-Presets}
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
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Header client origin unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    [CapyRowPointer]::Down('mouse',$from.x,$from.y)
    [CapyRowPointer]::Move([int]($origin.x+($zone.x+$zone.width*.5)*$scale),[int]($origin.y+($zone.y+$zone.height*.5)*$scale))
    Wait-Until {try{((Control 'title-bar').Current.HelpText|ConvertFrom-Json).preview.target}catch{$false}} 'Header tool insertion has no target'
    [CapyRowPointer]::Up();Wait-Until {(Model).picker} 'Header tool picker did not open'
    $presets=@((Model).picker.choices|Where-Object {$_.control.kind -eq 'brush'})
    if($presets.Count -ne 65 -or @($presets.control.id|Select-Object -Unique).Count -ne 65){throw 'Native customization lost a built-in brush preset'}
    foreach($name in @('Marker','Spray','Dual Texture')){
        $preset=@($presets|Where-Object label -eq $name)
        if($preset.Count -ne 1){throw "Named leaf preset is missing: $name"}
        $expectedIcon=@{'Marker'='marker';'Spray'='spray';'Dual Texture'='decoration'}[$name]
        if($preset[0].icon -ne $expectedIcon){throw "Named leaf preset lost its icon: $name"}
        (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($name)
        Wait-Until {(Model).picker.query -eq $name} "Preset search did not reach shared state: $name"
        $native=Control ('picker-choice-brush-'+$preset[0].control.id) -Arranged
        if($native.Current.Name -ne $name -or !$native.Current.IsEnabled){throw "Native preset is not discoverable: $name"}
    }
    (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
    Wait-Until {(Model).picker.query -eq ''} 'Preset search did not clear'
    $results+=@{brush_presets=65;named_leaves=@('Marker','Spray','Dual Texture')}
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
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width $width -Height 760
        try{Wait-Until {$null -ne ($script:hiddenZone=Hidden-Zone $entry.id)} 'Not hidden' 3;$zone=$script:hiddenZone;break}catch{}
    }
    if($null -eq $zone){throw 'The grouped title-bar tool never overflowed'}
    Capture 'header-overflow' -WithModel -Composed
    foreach($device in @('mouse','pen','touch')){$results+=Overflow-Variation $entry.id $zone $device}
    [IO.File]::WriteAllText((Join-Path $run 'results.json'),($results|ConvertTo-Json -Depth 5),[Text.UTF8Encoding]::new($false))
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    [pscustomobject]@{theme=$Theme;toolbar='passed';header='passed';header_overflow='passed';devices=@('mouse','pen','touch');eraser_leaf='passed';merged_presets='passed';brush_presets=65;checked_choices='passed';layout_unchanged='passed';evidence=$run}|ConvertTo-Json
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{[CapyRowPointer]::Dispose();Exit-CapyEnvironment}
