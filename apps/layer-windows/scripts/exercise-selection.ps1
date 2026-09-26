param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/selection/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
$ControlType=[System.Windows.Automation.ControlType]
Add-Type -AssemblyName System.Drawing
Add-Type -Name SelectionDpi -Namespace Capy -MemberDefinition '[DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);[DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,int[] point);'
function Dip($element){$element.Current.BoundingRectangle.Height/([Capy.SelectionDpi]::GetDpiForWindow($review.MainWindowHandle)/96.)}
function Outline([string]$Name,[int]$Left,[int]$Right,[int]$Y){
    Capture $Name
    $origin=[int[]]@(0,0);$null=[Capy.SelectionDpi]::ClientToScreen($review.MainWindowHandle,$origin)
    $image=[System.Drawing.Bitmap]::new((Join-Path $run ($Name+'.png')))
    try{
        $marked=0
        for($x=$Left;$x -le $Right;$x++){
            $ink=$false
            for($dy=-1;$dy -le 1;$dy++){if($image.GetPixel($x-$origin[0],$Y+$dy-$origin[1]).R -lt 200){$ink=$true}}
            if($ink){$marked++}
        }
        $marked/[Math]::Max(1,$Right-$Left+1)
    }finally{$image.Dispose()}
}
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
    do{try{if(& $Condition){return}}catch [System.Windows.Automation.ElementNotAvailableException]{};$review.Refresh();if($review.HasExited){throw 'Owned selection review exited unexpectedly'};Start-Sleep -Milliseconds 40}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    foreach($entry in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){if(!$entry.Current.IsOffscreen){return $entry}}
}
function Desktop-Find([string]$Value,$Type){
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Value),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))
    foreach($scope in @($root,[System.Windows.Automation.AutomationElement]::RootElement)){
        $hit=$scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
        if($hit){return $hit}
    }
}
function Control([string]$Id,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Type $Type;$null -ne $hit.item} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Center($element){$b=$element.Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id}
function Tools{(Model).state.layer_tools}
function Key([uint16]$Code){
    Wait-Until {(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Drawing canvas stayed disabled'
    (Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::KeyAt($Code,$paper.x,$paper.y)
}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {
        $found.switch=Find ('workspace-switch-'+$Name.ToLowerInvariant());$found.menu=Find 'header-workspace-menu'
        $found.switch -or $found.menu
    } "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type $ControlType::MenuItem}
    Wait-Until {try{$switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();$true}catch{$false}} "$Name switch stayed unavailable" 20
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Canvas-Points{
    $area=(Model).state.camera.work_area;$bounds=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
    $script:center=@{x=[int]($bounds.X+$area[0]+$area[2]/2);y=[int]($bounds.Y+$area[1]+$area[3]/2)}
    $script:paper=@{x=$center.x;y=$center.y-90}
}
function Drag($from,$to,[string]$Device='mouse'){
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    for($i=1;$i -le 12;$i++){[CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$i/12),[int]($from.y+($to.y-$from.y)*$i/12));Start-Sleep -Milliseconds 12}
    [CapyRowPointer]::Up()
}
function Tile-Id([string]$Command){
    foreach($panel in @((Model).panels)){foreach($tile in @($panel.tiles)){if($tile.control.kind -eq 'command' -and $tile.control.command -eq $Command){return "tile-$($panel.id)-$($tile.id)"}}}
    throw "No $Command tile"
}
function Header-Id([string]$Command){
    foreach($zone in @((Model).header.model.zones)){foreach($entry in @($zone)){if($entry.item.control.command -eq $Command){return $entry.id}}}
}
function Header-Icon([string]$Id){((Model).header.items|Where-Object id -eq $Id).icon}
function Menu-Items{
    $items=[System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::MenuItem))
    @($items|Where-Object {$_.Current.ProcessId -eq $review.Id -and !$_.Current.IsOffscreen})
}
function Close-Menu{
    for($i=0;$i -lt 3 -and @(Menu-Items).Count;$i++){[CapyRowPointer]::Key([uint16]0x1B);Start-Sleep -Milliseconds 150}
    Wait-Until {!@(Menu-Items).Count} 'Menu did not close'
}
function Check-Modes([string[]]$Expected,[bool]$Compact){
    $tops=@()
    foreach($id in $Expected){
        $button=Control ('tool-action-'+$id) -Type $ControlType::Button
        $command=Command $id
        if($button.Current.Name -ne $command.label){throw "Mode $id is not named by the shared label"}
        $height=Dip $button;$want=if($Compact){36}else{44}
        if([Math]::Abs($height-$want) -gt 1.5){throw "Mode $id is $height px, expected $want"}
        $tops+=$button.Current.BoundingRectangle.Y
        $button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {(Command $id).selected -and (Find ('tool-action-'+$id)).Current.ItemStatus -eq 'Selected'} "Mode $id did not select"
    }
    if(@($tops|Where-Object {[Math]::Abs($_-$tops[0]) -gt 1}).Count){throw 'Selection modes do not share one row'}
    foreach($id in @('selection_new','selection_add','selection_subtract','selection_intersect')){
        if($Expected -notcontains $id -and (Find ('tool-action-'+$id))){throw "Unexpected mode $id"}
    }
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned selection review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Selection review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Photo' 'builtin:workspace:photographer'
    foreach($command in @('rectangle_select','ellipse_select','polygon_select','color_select')){
        if(!@((Model).panels|ForEach-Object {$_.tiles}|Where-Object {$_.control.command -eq $command}).Count){throw "Photo has no $command tool"}
    }
    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    if(Find 'canvas-fit'){Invoke 'canvas-fit'};Start-Sleep -Milliseconds 300;Canvas-Points
    $select=Header-Id 'select'
    if(!$select -or (Header-Id 'lasso')){throw 'Sketch did not replace Lasso with the Select opener'}
    Tap (Center (Control ('header-item-'+$select)))
    Wait-Until {(Tools).tool -eq 'select' -or (Model).state.customization.drawer} 'Select did not activate the remembered selection tool'
    Start-Sleep -Milliseconds 400
    if(!(Model).state.customization.drawer){Tap (Center (Control ('header-item-'+$select)))}
    Wait-Until {(Model).state.customization.drawer.anchor.id -eq $select} 'Select did not open its drawer'
    if((ConvertTo-Json -InputObject (Model).state.customization.drawer.columns -Compress) -ne '[["tools"],["tool_settings"]]'){throw 'Select drawer columns differ from the shared drawer'}
    $choices=@((Model).state.tool_set.subtools)
    if($choices.Count -ne 8){throw "Expected eight selection tools, found $($choices.Count)"}
    $devices=@('mouse','touch','pen')
    for($i=0;$i -lt $choices.Count;$i++){
        $choice=$choices[$i];$row=Control ('tool-subtool-'+$i)
        if((Dip $row) -lt 43.5){throw "$($choice.label) row is shorter than 44 px"}
        Tap (Center $row) $devices[$i%3]
        Wait-Until {(Model).state.tool_set.subtools[$i].selected} "$($choice.label) did not select"
        Wait-Until {(Header-Icon $select) -eq $choice.icon} "Select opener did not remember $($choice.label)"
        if(!(Model).state.customization.drawer){throw "$($choice.label) closed the Select drawer"}
        $brush=$choice.icon -eq 'selection-brush';$tonal=$choice.icon -eq 'tonal-select'
        $expected=if($brush){@('selection_add','selection_subtract')}else{@('selection_new','selection_add','selection_subtract','selection_intersect')}
        Check-Modes $expected $tonal
        $setting=if($brush){'selection_brush_size'}elseif($tonal){'selection_feather'}else{'selection_feather'}
        $null=Control ('tool-setting-'+$setting) -Type $ControlType::Edit
        if(!$brush -and !$tonal){$null=Control 'tool-action-selection_antialias' -Type $ControlType::CheckBox}
        if($tonal){
            $null=Control 'tool-choice-tonal-tones'
            if(Find 'selection-actions-menu'){throw 'Tonal range showed Selection Actions'}
            if((Dip (Control 'tool-subtool-0')) -gt 37.5){throw 'Tool rows did not shrink for Tonal range'}
            $custom=@((Model).state.tool_extra|Where-Object {$_.Choice.id -eq 'tonal-tones'})[0].Choice.items
            $index=[Array]::FindIndex([object[]]$custom,[Predicate[object]]{param($x)$x.icon -eq 'tonal-custom'})
            if($index -lt 0){throw 'Tone bar lacks Custom'}
            Invoke ('tool-choice-tonal-tones-'+$index)
            Wait-Until {Find 'tool-setting-range'} 'Custom tonal range did not show the two-handle range'
            $lower=((Model).state.tool_settings|Where-Object id -eq 'tonal_lower').value
            $track=Control 'tool-setting-range-track'
            $b=$track.Current.BoundingRectangle
            Drag @{x=[int]($b.X+10);y=[int]($b.Y+$b.Height/2)} @{x=[int]($b.X+$b.Width*.3);y=[int]($b.Y+$b.Height/2)}
            Wait-Until {((Model).state.tool_settings|Where-Object id -eq 'tonal_lower').value -ne $lower} 'Range handle drag did not move the lower endpoint'
            Capture 'tonal-settings'
        }else{
            if(!$brush){$null=Control 'selection-actions-menu'}
        }
    }
    Tap (Center (Control 'tool-subtool-0'))
    Wait-Until {(Model).state.tool_set.subtools[0].selected} 'Rectangle Select did not restore'
    Capture 'select-drawer'
    Invoke 'selection-actions-menu'
    Wait-Until {@(Menu-Items).Count -gt 0} 'Selection Actions did not open a menu'
    $rows=@(Menu-Items)
    foreach($label in @('Load Selection','Replace Selection Layer from Current Selection')){
        $row=$rows|Where-Object {$_.Current.Name -eq $label}|Select-Object -First 1
        if(!$row -or $row.Current.IsEnabled){throw "$label must be a disabled leaf without saved layers"}
        $pattern=$null;if($row.TryGetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern,[ref]$pattern)){throw "$label shows a submenu arrow"}
    }
    foreach($label in @("Grow$([char]0x2026)","Shrink$([char]0x2026)")){if(!@($rows|Where-Object {$_.Current.Name -eq $label}).Count){throw "Selection Actions lacks $label"}}
    if(@($rows|Where-Object {$_.Current.Name -eq 'Modify'}).Count){throw 'Selection Actions still nests Modify'}
    Close-Menu

    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    Key 0x1B
    Wait-Until {!(Model).state.customization.drawer} 'Escape did not close the Select drawer'

    Switch-Workspace 'Photo' 'builtin:workspace:photographer'
    if(Find 'canvas-fit'){Invoke 'canvas-fit'};Start-Sleep -Milliseconds 300;Canvas-Points
    Invoke (Tile-Id 'rectangle_select')
    Wait-Until {(Tools).tool.selection.kind -eq 'rectangle' -and (Command 'select_all').enabled} 'Photo Rectangle Select did not activate'
    if((Tools).has_selection){
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Select'
        $deselect=@{item=$null};Wait-Until {$deselect.item=@(Menu-Items)|Where-Object {$_.Current.AutomationId -eq 'deselect'}|Select-Object -First 1;$deselect.item} 'Select menu has no Deselect'
        $deselect.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {!(Tools).has_selection} 'Deselect did not clear the tonal selection'
    }
    $a=@{x=$center.x-80;y=$center.y-60};$b=@{x=$center.x+80;y=$center.y+60}
    Drag $a $b
    Wait-Until {(Tools).has_selection} 'Rectangle selection did not complete'
    [CapyRowPointer]::Hover($center.x+160,$center.y+120)
    Wait-Until {(Outline 'rectangle' ($center.x-60) ($center.x+60) ($center.y-60)) -gt .3} 'Rectangle outline was not presented'
    [CapyRowPointer]::Hold(0x12,$true)
    try{Drag @{x=$center.x-20;y=$center.y-20} @{x=$center.x+20;y=$center.y+20}}finally{[CapyRowPointer]::Hold(0x12,$false)}
    [CapyRowPointer]::Hover($center.x+160,$center.y+120)
    Wait-Until {(Outline 'subtract' ($center.x-12) ($center.x+12) ($center.y-20)) -gt .3} 'Alt-latched Subtract hole was not presented without further input'
    if(@(Menu-Items).Count){throw 'Releasing Alt opened a menu'}
    if(!(Tools).has_selection){throw 'Subtracting a hole cleared the selection'}
    Invoke (Tile-Id 'undo')
    Wait-Until {(Outline 'undo-subtract' ($center.x-12) ($center.x+12) ($center.y-20)) -lt .1} 'Undo did not remove only the Subtract hole'
    if(!(Tools).has_selection){throw 'Alt-latched Subtract was not its own undo step'}
    Invoke (Tile-Id 'undo')
    Wait-Until {!(Tools).has_selection} 'Rectangle selection was not one undo step'
    Invoke (Tile-Id 'redo');Wait-Until {(Tools).has_selection} 'Redo did not restore the rectangle'
    Invoke (Tile-Id 'redo')
    Wait-Until {(Outline 'redo-subtract' ($center.x-12) ($center.x+12) ($center.y-20)) -gt .3} 'Redo did not restore the Subtract hole'

    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Select'
    $grow=@{item=$null};Wait-Until {$grow.item=@(Menu-Items)|Where-Object {$_.Current.Name -eq "Grow$([char]0x2026)"}|Select-Object -First 1;$grow.item} 'Select menu has no Grow'
    $grow.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Tools).selection_resize} 'Grow did not open the shared resize draft'
    $distance=Control 'selection-resize-distance' -Type $ControlType::Edit
    $distance.SetFocus()
    $distance.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('8')
    $null=Control 'selection-resize-dialog'
    Capture 'grow-dialog'
    $apply=Desktop-Find 'Apply' $ControlType::Button
    $apply.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Tools).selection_resize} 'Apply did not finish the resize draft' 20
    if(!(Tools).has_selection){throw 'Grow lost the selection'}
    Wait-Until {(Command 'select_all').enabled} 'Grow did not finish' 20

    $family=$null
    foreach($panel in @((Model).panels)){foreach($tile in @($panel.tiles)){foreach($option in @($tile.component.options)){
        if($option.Choice -and @($option.Choice.items|Where-Object label -eq 'Tonal range').Count){$family=$option.Choice}
    }}}
    if(!$family){throw 'Tool Options has no selection family choice'}
    Invoke ('toolbar-choice-'+$family.id)
    $tonalItem=@{item=$null}
    Wait-Until {$tonalItem.item=@(Menu-Items)|Where-Object {$_.Current.Name -eq 'Tonal range'}|Select-Object -First 1;$tonalItem.item} 'Tool Options family menu lacks Tonal range'
    $pattern=$null
    if($tonalItem.item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    else{$tonalItem.item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
    Wait-Until {(Tools).tool.selection.kind -eq 'tonal'} 'Tool Options did not choose Tonal range'
    $tones=$null
    foreach($panel in @((Model).panels)){foreach($tile in @($panel.tiles)){foreach($option in @($tile.component.options)){if($option.Choice.id -eq 'tonal-tones'){$tones=$option.Choice}}}}
    if(!$tones){throw 'Tool Options lacks the tone bar'}
    $index=[Array]::FindIndex([object[]]@($tones.items),[Predicate[object]]{param($x)$x.icon -eq 'tonal-custom'})
    Invoke ('toolbar-segment-tonal-tones-'+$index)
    Wait-Until {Find 'toolbar-range'} 'Custom tonal range did not add the Tool Options range'
    $upper=((Model).state.tool_settings|Where-Object id -eq 'tonal_upper').value
    $track=(Control 'toolbar-range-track').Current.BoundingRectangle
    Drag @{x=[int]($track.X+$track.Width-10);y=[int]($track.Y+$track.Height/2)} @{x=[int]($track.X+$track.Width*.7);y=[int]($track.Y+$track.Height/2)}
    Wait-Until {((Model).state.tool_settings|Where-Object id -eq 'tonal_upper').value -ne $upper} 'Tool Options range drag did not move the upper endpoint'
    Capture 'toolbar-tonal'
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    Key 0x1B
    Invoke (Tile-Id 'rectangle_select')
    Wait-Until {(Tools).tool.selection.kind -eq 'rectangle' -and (Command 'select_all').enabled} 'Rectangle Select did not restore after Tonal range'

    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    $artwork=(Model).state.colors.foreground.rgba|ConvertTo-Json -Compress
    Key 0x51
    Wait-Until {(Tools).quick_mask} 'Q did not enter Quick Mask'
    $row=Control 'layer-row-0'
    Wait-Until {(Find 'layer-0-thumbnail').Current.ItemStatus -eq 'Ready'} 'Quick Mask thumbnail did not load' 45
    $null=Control 'layer-0-load'
    if((Control 'layer-0-visibility').Current.Name -notlike '*selection overlay*'){throw 'Quick Mask eye does not describe the overlay'}
    if(!(Tools).mask_editing){throw 'Quick Mask did not publish mask painting colors'}
    $mask=(Tools).mask_editing.colors.foreground.rgba|ConvertTo-Json -Compress
    Key 0x44
    Wait-Until {((Tools).mask_editing.colors.foreground.rgba|ConvertTo-Json -Compress) -ne $mask} 'D did not reset mask painting colors'
    if(((Model).state.colors.foreground.rgba|ConvertTo-Json -Compress) -ne $artwork){throw 'Resetting mask colors changed artwork colors'}
    [CapyRowPointer]::RightClick((Center (Control 'layer-0-name')).x,(Center (Control 'layer-0-name')).y)
    Wait-Until {@(Menu-Items|Where-Object {$_.Current.Name -eq 'Save as Selection Layer'}).Count} 'Quick Mask row menu lacks Save as Selection Layer'
    Capture 'quick-mask-menu'
    Close-Menu
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    Key 0x1B
    Wait-Until {!(Tools).quick_mask} 'Escape did not leave Quick Mask'
    if(((Model).state.colors.foreground.rgba|ConvertTo-Json -Compress) -ne $artwork){throw 'Leaving Quick Mask changed artwork colors'}

    $before=@((Model).state.layers|ForEach-Object {$_.id})
    Invoke 'layer-new-selection'
    $created=@{id=$null}
    Wait-Until {$layer=@((Model).state.layers|Where-Object {$_.selection_layer -and $before -notcontains $_.id})[0];$created.id=$layer.id;$null -ne $layer} 'New Selection Layer did not add a row'
    $id=$created.id
    Wait-Until {(Find ('layer-'+$id+'-rename')).Current.HasKeyboardFocus} 'New Selection Layer did not focus inline rename'
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Key([uint16]0x0D)
    Wait-Until {!(Find ('layer-'+$id+'-rename'))} 'Rename did not finish'
    Start-Sleep -Milliseconds 400
    if(@((Model).state.layers|Where-Object {$_.selection_layer -and $before -notcontains $_.id}).Count -ne 1){throw 'Confirming the name created another selection layer'}
    Wait-Until {(Find ('layer-'+$id+'-thumbnail')).Current.ItemStatus -eq 'Ready'} 'Selection layer thumbnail did not load' 45
    if((Control ('layer-'+$id+'-content')).Current.Name -ne 'Edit selection layer'){throw 'Selection layer content lacks its edit name'}
    $load=Control ('layer-'+$id+'-load')
    $tooltip=((Model).state.layers|Where-Object id -eq $id).load_selection_tooltip
    if($load.Current.Name -ne $tooltip){throw 'Load button does not use the shared tooltip'}
    $load.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Tools).has_selection -and !(Model).state.layers.Where({$_.id -eq $id})[0].editing} 'Load did not load coverage and return to artwork'
    Capture 'selection-layer'
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Selection review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Selection review wrote to stderr'}
    @{tools=@($choices|ForEach-Object {$_.label});selection_layer=$id}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'result.json')
    Write-Output "Selection acceptance passed: $run"
}catch{
    try{Capture 'failure'}catch{}
    try{@{tools=(Tools);drawer=(Model).state.customization.drawer;subtools=(Model).state.tool_set.subtools}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    Set-Content -LiteralPath (Join-Path $run 'failure.txt') -Value ($_|Out-String)
    throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
