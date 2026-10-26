param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse',[ValidateSet('paint','sketch','photo')][string]$Workspace='paint',[switch]$Catalog,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','catalog-only','catalog-only-light','background','options','options-light','options-pen','options-touch','menus','menus-light','zen','zen-workspace')][string]$Journey='full')
$ErrorActionPreference='Stop'
$catalogJourney=$Journey -in @('catalog-only','catalog-only-light')
if($Journey -eq 'catalog-only-light'){$Theme='light'}
if($Journey -eq 'options-light'){$Theme='light'}
if($Journey -eq 'menus-light'){$Theme='light'}
if($Journey -eq 'options-pen'){$Device='pen'}
if($Journey -eq 'options-touch'){$Device='touch'}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$CapyPopups=$true
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/header/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null

function Header { (Model).header.model }
function HeaderJson { Header | ConvertTo-Json -Depth 30 -Compress }
function Presentation { try { (Control 'title-bar').Current.ItemStatus | ConvertFrom-Json } catch {} }
function Gesture { try { (Control 'title-bar').Current.HelpText | ConvertFrom-Json } catch {} }
function At([string]$Id) {
    $box=(Control $Id).Current.BoundingRectangle
    if($box.IsEmpty -or $box.Width -le 0 -or $box.Height -le 0){throw "Unarranged control: $Id"}
    @{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}
}
function Check-BackgroundContext {
    Edit-Header
    $box=(Control 'title-bar').Current.BoundingRectangle
    $height=(Presentation).height*[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $occupied=@(Entries|ForEach-Object {if($item=Find ('header-item-'+$_.id)){$item.Current.BoundingRectangle}})
    if($capy=Find 'zen-capy'){$occupied+=,$capy.Current.BoundingRectangle}
    $at=$null
    foreach($step in 1..100){
        $x=$box.X+$box.Width*$step/101;$y=$box.Y+$height/2
        if(!@($occupied|Where-Object {$_.Contains($x,$y)}).Count){$at=@{x=[int]$x;y=[int]$y};break}
    }
    if(!$at){throw 'Title bar has no unused background'}
    $before=HeaderJson
    foreach($pointer in 'mouse','touch','pen'){
        [CapyRowPointer]::Down($pointer,$at.x,$at.y)
        Start-Sleep -Milliseconds 900
        if($pointer -eq 'mouse'){
            if((Gesture).menu_open){throw 'Mouse title background hold opened a menu'}
        }else{Wait-Until {(Gesture).menu_open} "$pointer title background hold did not open its context menu"}
        [CapyRowPointer]::Up()
        if($pointer -ne 'mouse'){
            if(!(Gesture).menu_open){throw 'Title background release lost the menu'}
            [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
            Wait-Until {!(Gesture).menu_open} 'Title background menu did not dismiss'
        }
        if((HeaderJson) -ne $before -or !(Model).header.editing){throw 'Holding the title background changed its layout'}
    }
    Invoke 'header-edit-cancel'
    Wait-Until {!(Model).header.editing} 'Title background check did not close the editor'
}
function Screen($Box) {
    $point=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($drawingWindow,[ref]$point)){throw 'Client origin unavailable'}
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    @{x=[int]($point.x+$Box.x*$scale);y=[int]($point.y+$Box.y*$scale)}
}
function WindowCommand([string]$Id) {
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    Invoke $Id
}
function Check-MenuHover([int]$Height) {
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](1400*$scale)) -Height ([int]($Height*$scale))
    Add-Type -AssemblyName System.Windows.Forms
    $work=[Windows.Forms.Screen]::FromHandle($drawingWindow).WorkingArea
    $windowWidth=[int](1400*$scale);$windowHeight=[int]($Height*$scale)
    $left=$work.Left+[int][Math]::Max(0,($work.Width-$windowWidth)/2);$top=$work.Top+[int]($work.Height/4)
    if(![CapyWindowExercise]::MoveWindow($drawingWindow,$left,$top,$windowWidth,$windowHeight,$true)){throw 'The owned menu window did not move'}
    Check-Geometry
    $type=[System.Windows.Automation.ControlType]::MenuItem
    $markers=@('new_document','undo','zoom_in')
    $headings=@('file','edit','view')
    $points=@($headings|ForEach-Object {At ('application-menu-'+$_)})
    foreach($at in $points){
        [CapyRowPointer]::Hover($at.x,$at.y);Start-Sleep -Milliseconds 350
        foreach($marker in $markers){if(Find $marker -Type $type -Visible){throw 'Hover opened a closed application menu'}}
    }
    $at=$points[0];[CapyRowPointer]::Down('mouse',$at.x,$at.y);[CapyRowPointer]::Up()
    Wait-Until {$null -ne (Find $markers[0] -Type $type -Visible)} 'Pointer click did not open File'
    foreach($index in 1,2,0){
        $at=$points[$index];[CapyRowPointer]::Hover($at.x,$at.y)
        Wait-Until {
            if(!(Find $markers[$index] -Type $type -Visible)){return $false}
            foreach($other in 0..2){if($other -ne $index -and (Find $markers[$other] -Type $type -Visible)){return $false}}
            return $true
        } ('Hover did not switch to '+$headings[$index])
        $heading=(Control ('application-menu-'+$headings[$index])).Current.BoundingRectangle
        $item=Control $markers[$index] -Type $type
        $popup=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($item)
        @{height=$Height;heading=$heading;item=$item.Current.BoundingRectangle;popup=$popup.Current.BoundingRectangle}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ('menu-hover-'+$Height+'-'+$headings[$index]+'-layout.json'))
        if($popup.Current.BoundingRectangle.Top -lt $heading.Bottom){throw 'Application menu popup overlaps its headings'}
        Capture ('menu-hover-'+$Height+'-'+$headings[$index]) -Composed
    }
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {foreach($marker in $markers){if(Find $marker -Type $type -Visible){return $false}};return $true} 'Escape did not dismiss the switched application menu'
    $at=$points[1];[CapyRowPointer]::Hover($at.x,$at.y);Start-Sleep -Milliseconds 350
    foreach($marker in $markers){if(Find $marker -Type $type -Visible){throw 'Hover reopened the application menu after Escape'}}
}
function Check-MenuPreferences([string]$Id) {
    Invoke $Id
    Wait-Until {$null -ne (Find 'settings' -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible)} 'Compact menu has no direct Preferences item'
    Capture ($Id+'-preferences') -Composed
    Invoke 'settings'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    Invoke 'CloseButton' -Within $dialog
    Wait-Until {!(Find 'Preferences' -Name) -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not release canvas'
}
function Check-Geometry {
    Wait-Until {
        $p=Presentation
        if(!$p -or !$p.geometry.items.Count){return $false}
        foreach($item in $p.geometry.items){
            if(!$p.editing -and $item.id -eq $p.geometry.capy_item){
                $capy=Find 'zen-capy' -Visible;if(!$capy){return $false}
                $bounds=$capy.Current.BoundingRectangle;$origin=(Control 'title-bar').Current.BoundingRectangle
                $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
                $actual=@{bounds=@{x=($bounds.Left-$origin.Left)/$scale;y=($bounds.Top-$origin.Top)/$scale;width=$bounds.Width/$scale;height=$bounds.Height/$scale}}
            }else{$actual=$p.actual_items|Where-Object id -eq $item.id|Select-Object -First 1}
            if(!$actual){return $false}
            foreach($axis in @('x','y','width','height')){
                if([Math]::Abs($actual.bounds.$axis-$item.bounds.$axis) -gt 1){return $false}
            }
        }
        return $true
    } 'Native header does not match shared geometry'
}
function Start-Review([string]$Phase) {
    $script:stderr=Join-Path $run ($Phase+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/header-review.json')
    Write-Output "Owned header review $($review.Id) ($Device)"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Header review did not start' 45
    $script:root=$native.window.Root;$script:drawingWindow=$native.window.Handle
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if($Phase -eq 'initial'){
        $workspaceId=@{sketch='builtin:workspace:painter';paint='builtin:workspace:illustrator';photo='builtin:workspace:photographer'}[$Workspace]
        $choice=(Model).windows_workspace.switcher|Where-Object id -eq $workspaceId|Select-Object -First 1
        if(!$choice){throw 'Requested workspace is missing'}
        if((Model).windows_workspace.id -ne $choice.id){
            $toggle=Find ('workspace-switch-'+$choice.id)
            if(!$toggle -or $toggle.Current.IsOffscreen){
                Invoke 'header-workspace-menu'
                $toggle=Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
            }
            $toggle.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
            Wait-Until {(Model).windows_workspace.id -eq $choice.id -and !(Model).windows_workspace.busy} 'Requested workspace did not open'
        }
        $bands=@((Model).state.workspace.layout.bands)
        if($Workspace -eq 'sketch' -and ((Header).size -ne 'medium' -or $bands.Count -ne 1 -or $bands[0].edge -ne 'left' -or $bands[0].alignment -ne 'center' -or (Model).state.workspace.layout.canvas_info.visible)){
            throw 'Sketch did not open the shared minimal titlebar layout'
        }
    }
    Check-Geometry
}
function Check-PenOrder {
    function BrushSetsMenu {
        $window=@((Model).application_menus|Where-Object id -eq 'window')[0].model
        $choice=@($window.sections|ForEach-Object {$_}|Where-Object {$_.action.type -eq 'customize' -and $_.action.action.type -eq 'set_panel_visible' -and $_.action.action.panel -eq 'brush_sets'})
        if($choice.Count -ne 1){throw 'The shared Window menu has no unique Brushes panel'}
        $choice[0]
    }
    function Show-BrushSets([bool]$Visible) {
        $choice=BrushSetsMenu
        if([bool]$choice.selected -ne $Visible){
            & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
            (Control $choice.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
            Wait-Until {[bool](BrushSetsMenu).selected -eq $Visible -and !(Find $choice.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible)} 'The Brushes panel did not acknowledge its visibility'
        }
        if($Visible){
            $active=@{group=$null}
            Wait-Until {$groups=@((Model).layout.groups|Where-Object active -eq 'brush_sets');if($groups.Count -ne 1 -or (Model).windows_workspace.busy){return $false};$active.group=$groups[0];$true} 'The Brushes panel did not become the active native group'
            $group=Control ('workspace-group-'+$active.group.id) -Arranged
            $first=@((Model).state.tool_panels.brush_sets.groups)[0]
            $row=Control ('brush-set-'+$first.icon) -Within $group -Arranged
            if($group.Current.ProcessId -ne $review.Id -or $row.Current.ProcessId -ne $review.Id){throw 'The arranged Brushes panel belongs to another review'}
            $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
            Wait-Until {
                $viewport=Native-Bounds ('panel-body-scroll-'+$active.group.id) $scale
                $row=Find ('brush-set-'+$first.icon) -Within $group -Visible
                $row -and $row.Current.ProcessId -eq $review.Id -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow -and !$row.Current.BoundingRectangle.IsEmpty -and $viewport.Contains($row.Current.BoundingRectangle)
            } 'The first Brushes category did not arrange inside its owned native viewport'
        }
    }
    $before=(Model).state;$groups=@($before.tool_panels.brush_sets.groups)
    $prior=@($groups|Where-Object selected);$pen=@($groups|Where-Object {$_.action.type -eq 'select_brush_set' -and $_.action.group -eq 'pen'})
    if($prior.Count -ne 1 -or $pen.Count -ne 1){throw 'The shared Brushes panel has no unique current and Pen categories'}
    $visible=[bool](BrushSetsMenu).selected
    Show-BrushSets $true
    try{
        $categories=@(foreach($choice in $groups){
            $button=Reveal ('brush-set-'+$choice.icon)
            if($button.Current.Name -ne $choice.label -or ($button.Current.ItemStatus -eq 'Selected') -ne [bool]$choice.selected){throw 'A native brush category has a stale caption or selection'}
            @{group=$choice.action.group;icon=$choice.icon;name=$button.Current.Name;selected=$choice.selected;bounds=$button.Current.BoundingRectangle;runtime_id=($button.GetRuntimeId() -join ':')}
        })
        $null=Reveal ('brush-set-'+$pen[0].icon)
        Invoke ('brush-set-'+$pen[0].icon)
        Wait-Until {$m=Model;$m.state.brush.tool -eq 'pen' -and @($m.state.tool_panels.brush_sets.groups|Where-Object {$_.action.group -eq 'pen' -and $_.selected}).Count -eq 1 -and (Control ('brush-set-'+$pen[0].icon)).Current.ItemStatus -eq 'Selected'} 'The Pen category did not activate'
        @{categories=$categories;selected_icon=$pen[0].icon;visual_review='required: shared medium icons, category captions and selected styling'}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'paint-categories-layout.json')
        Capture 'paint-categories' -WithModel -Composed
    }catch{
        $failure=$_
        try{
            $model=Model;$active=@($model.layout.groups|Where-Object active -eq 'brush_sets')
            $row=Find ('brush-set-'+$pen[0].icon)
            $group=if($active.Count -eq 1){Find ('workspace-group-'+$active[0].id)}
            $viewport=if($active.Count -eq 1){Native-Bounds ('panel-body-scroll-'+$active[0].id) ([CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.)}
            @{failure=($failure|Out-String);groups=$active;categories=$model.state.tool_panels.brush_sets.groups;row=@(if($row){Control-Trace $row});viewport=$viewport;native_group=@(if($group){Control-Trace $group})}|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'paint-category-failure-receipt.json')
            Capture 'paint-category-failure' -WithModel -Composed
        }catch{[IO.File]::WriteAllText((Join-Path $run 'paint-category-diagnostic-error.txt'),($_|Out-String))}
        throw $failure
    }finally{Show-BrushSets $visible}
    try{
    $hint=@((Model).state.commands|Where-Object id -eq 'pen')[0]
    if(!$hint.shortcut){throw 'The shared Pen command has no shortcut hint'}
    $tooltip=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
    function PenTooltip {
        foreach($window in Owned-ProcessWindows $review){
            foreach($tip in $window.Root.FindAll([System.Windows.Automation.TreeScope]::Subtree,$tooltip)){
                if(!$tip.Current.IsOffscreen -and $tip.Current.ProcessId -eq $review.Id){return $tip}
            }
        }
    }
    $away=At 'drawing-canvas';[CapyRowPointer]::Hover($away.x,$away.y)
    $at=At (Tool-Tile 'pen');[CapyRowPointer]::Hover($at.x,$at.y)
    Wait-Until {$tip=PenTooltip;$tip -and $tip.Current.Name -eq $hint.tooltip} 'The native Pen tooltip did not show its shared shortcut'
    Capture 'pen-shortcut-tooltip' -Composed
    [CapyRowPointer]::Hover($away.x,$away.y)
    Wait-Until {!(PenTooltip)} 'The Pen shortcut tooltip did not dismiss'
    $state=(Model).state;$items=@($state.tool_set.subtools);$original=$state.brush.preset
    $rough=@(0..($items.Count-1)|Where-Object {$items[$_].preview -eq 28})
    $blotty=@(0..($items.Count-1)|Where-Object {$items[$_].preview -eq 33})
    $restore=@(0..($items.Count-1)|Where-Object {$items[$_].preview -eq $original})
    if($rough.Count -ne 1 -or $blotty.Count -ne 1 -or $restore.Count -ne 1 -or $blotty[0] -ne $rough[0]+1){throw 'Blotty Ink does not follow Rough G-Pen in the shared Pen group'}
    $target=Control ('tool-subtool-'+$blotty[0])
    $walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$scroller=$walker.GetParent($target);$scroll=$null
    while($scroller -and !$scroller.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$scroller=$walker.GetParent($scroller)}
    if(!$scroll){throw 'The native Pen group has no scroll provider'}
    $scrollPercent=$scroll.Current.VerticalScrollPercent
    try{
        $itemScroll=$null
        if($target.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$itemScroll)){$itemScroll.ScrollIntoView()}else{$target.SetFocus()}
        $records=@(foreach($index in @($rough[0],$blotty[0])){
            $button=Control ('tool-subtool-'+$index) -Arranged;$bounds=$button.Current.BoundingRectangle
            if($button.Current.Name -ne $items[$index].label -or !$root.Current.BoundingRectangle.Contains($bounds)){throw 'Native Pen button has a stale label or invalid bounds'}
            @{index=$index;preview=$items[$index].preview;name=$button.Current.Name;bounds=$bounds;runtime_id=($button.GetRuntimeId() -join ':')}
        })
        if($records[1].bounds.Top -lt $records[0].bounds.Bottom-1){throw 'Native Blotty Ink is not arranged below Rough G-Pen'}
        Invoke ('tool-subtool-'+$blotty[0])
        Wait-Until {$m=Model;$m.state.brush.preset -eq 33 -and $m.state.tool_set.subtools[$blotty[0]].selected -and (Control ('tool-subtool-'+$blotty[0])).Current.ItemStatus -eq 'Selected'} 'Native Blotty Ink selection did not reach the shared brush'
        @{items=$records;selected=(Model).state.brush.preset;previous=$original;shortcut_tooltip=$hint.tooltip;raw_review='required: Rough G-Pen and Blotty Ink order, captions, preview and selected styling'}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'blotty-ink-order-layout.json')
        Capture 'blotty-ink-order' -WithModel -Composed
    }finally{
        try{
            Invoke ('tool-subtool-'+$restore[0])
            Wait-Until {(Model).state.brush.preset -eq $original -and (Control ('tool-subtool-'+$restore[0])).Current.ItemStatus -eq 'Selected'} 'The original Pen preset did not restore'
        }finally{
            if($scroll.Current.VerticallyScrollable){Scroll-Position $target $scrollPercent}
        }
    }
    }finally{
        if($prior[0].action.group -ne 'pen'){Invoke (Tool-Tile $before.brush.tool);Wait-Until {(Model).state.brush.preset -eq $before.brush.preset -and @((Model).state.tool_panels.brush_sets.groups|Where-Object {$_.action.group -eq $prior[0].action.group -and $_.selected}).Count -eq 1} 'The original brush category did not restore'}
    }
}
function Check-CompactWorkspace {
    $before=$root.Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](1200*$scale)) -Height ([int](700*$scale))
    Wait-Until {$group=Find 'workspace-switcher' -Visible;$group -and [Math]::Abs($root.Current.BoundingRectangle.Width-1200*$scale) -lt 2} 'Full workspace switcher did not appear at the ordinary wide width'
    Check-Geometry
    $view=Model;$group=Control 'workspace-switcher' -Arranged;$bounds=$group.Current.BoundingRectangle;$contained=[Windows.Rect]::new($bounds.X,$bounds.Y,$bounds.Width,$bounds.Height);$contained.Inflate($scale,$scale)
    $choices=@($view.windows_workspace.switcher_display);$records=@();$errors=[Collections.Generic.List[string]]::new()
    if($view.windows_active_tag -ne 'en' -or $choices.Count -ne 3 -or @($choices|Where-Object {$_.title -notin @('Sketch','Paint','Photo')}).Count){throw 'Workspace width fixture requires the three short English starting captions'}
    if($group.Current.Name -ne 'Task workspaces' -or $group.Current.ControlType -ne [System.Windows.Automation.ControlType]::Group){$errors.Add('Workspace switcher does not expose its named whole-component group')}
    foreach($choice in $choices){
        $id='workspace-switch-'+$choice.id;$button=Control $id -Arranged;$box=$button.Current.BoundingRectangle
        $texts=@($button.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))|Where-Object {!$_.Current.IsOffscreen})
        if($button.Current.Name -ne $choice.title -or !$contained.Contains($box)){$errors.Add("Workspace button is stale or outside its group: $id")}
        if($texts.Count -ne 1){$errors.Add("Workspace caption is not observable as one native Text provider: $id")}
        $captions=@(foreach($text in $texts){
            $caption=$text.Current.BoundingRectangle;$inside=[Windows.Rect]::new($box.X,$box.Y,$box.Width,$box.Height);$inside.Inflate($scale,$scale)
            $padding=($box.Width-$caption.Width)/$scale;$left=($caption.Left-$box.Left)/$scale;$right=($box.Right-$caption.Right)/$scale
            if($text.Current.Name -ne $choice.title -or $caption.Width -le 0 -or $caption.Height -le 0 -or !$inside.Contains($caption) -or [Math]::Abs($padding-16) -gt 1 -or [Math]::Abs($left-8) -gt 1 -or [Math]::Abs($right-8) -gt 1){$errors.Add("Workspace caption does not fit its actual button padding: $id")}
            @{name=$text.Current.Name;runtime_id=($text.GetRuntimeId() -join ':');bounds=$caption;horizontal_padding_dip=$padding;left_inset_dip=$left;right_inset_dip=$right}
        })
        $records+=@{id=$id;name=$button.Current.Name;runtime_id=($button.GetRuntimeId() -join ':');bounds=$box;captions=$captions}
    }
    $options=Control 'workspace-switcher-options' -Arranged;$optionsBox=$options.Current.BoundingRectangle
    if(!$contained.Contains($optionsBox)){$errors.Add('Workspace options is outside the whole-component group')}
    @{dpi=96*$scale;group=@{name=$group.Current.Name;type=$group.Current.ControlType.ProgrammaticName;bounds=$bounds;runtime_id=($group.GetRuntimeId() -join ':')};choices=$records;options=@{name=$options.Current.Name;bounds=$optionsBox};errors=@($errors);visual_review='required: inspect the whole switcher, all captions and fixed options in the composed image'}|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'workspace-switcher-sizing-layout.json')
    Capture 'workspace-switcher-sizing' -Composed
    if($errors.Count){throw ($errors -join '; ')}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](960*$scale)) -Height ([int](700*$scale))
    Wait-Until {$button=Find 'header-workspace-menu' -Visible;$button -and [Math]::Abs($root.Current.BoundingRectangle.Width-960*$scale) -lt 2} 'Compact Workspaces did not appear at the ordinary narrow width'
    Check-Geometry
    $button=Control 'header-workspace-menu' -Arranged;$box=$button.Current.BoundingRectangle
    $entry=@(Entries|Where-Object {$_.item.kind -eq 'workspaces'})[0]
    $presentation=Presentation;$item=@($presentation.actual_items|Where-Object id -eq $entry.id)[0]
    $texts=@($button.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))|Where-Object {!$_.Current.IsOffscreen})
    @{dpi=96*$scale;name=$button.Current.Name;bounds=$box;actual=$item;presentation=$presentation;caption_providers=@($texts|ForEach-Object {@{name=$_.Current.Name;bounds=$_.Current.BoundingRectangle}});visual_review='required'}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'compact-workspaces-layout.json')
    Capture 'compact-workspaces' -Composed
    if([Math]::Abs($box.Width/$scale-144) -gt 1 -or [Math]::Abs($box.Height/$scale-$item.bounds.height) -gt 1){throw 'Compact Workspaces does not retain its caption-sized width and tile height'}
    foreach($text in $texts){if($text.Current.Name -ne $button.Current.Name -or !$box.Contains($text.Current.BoundingRectangle)){throw 'Compact Workspaces caption is stale or outside its button'}}
    Invoke 'header-workspace-menu'
    $choice=@((Model).windows_workspace.switcher|Where-Object id -eq (Model).windows_workspace.id)[0]
    $null=Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {!(Find $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Visible)} 'Compact Workspaces menu did not dismiss'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]$before.Width) -Height ([int]$before.Height)
    Wait-Until {[Math]::Abs($root.Current.BoundingRectangle.Width-$before.Width) -lt 2 -and [Math]::Abs($root.Current.BoundingRectangle.Height-$before.Height) -lt 2} 'Header window did not restore its original dimensions'
    Check-Geometry
}

function Edit-Header {
    WindowCommand 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Titlebar editor did not open'
    Check-Geometry
    $null=Control 'header-edit-done'
}
function Check-WorkspaceOptions {
    $entry=@(Entries|Where-Object {$_.item.kind -eq 'workspaces'})[0]
    if(!$entry){return}
    $id='header-select-'+$entry.id
    $source=Control $id -Arranged;$source.SetFocus()
    Wait-Until {!(Find 'customize_workspace_ui' -Visible) -and $source.Current.HasKeyboardFocus -and [CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Application menu did not dismiss and return focus to the owned header item'
    $box=$source.Current.BoundingRectangle
    $at=@{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}
    $before=HeaderJson;$active=(Model).windows_workspace.id
    [CapyRowPointer]::RightClick($at.x,$at.y)
    Wait-Until {$null -ne (Find ('workspace-switcher-show-'+$active))} 'Titlebar editor workspace context did not open visibility options'
    $menu=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent((Control ('workspace-switcher-show-'+$active)))
    if(Find 'Show in top bar' -Name -Within $menu -Type ([System.Windows.Automation.ControlType]::MenuItem)){throw 'Editor visibility checklist has a redundant heading'}
    if(Find 'Customize Title Bar' -Name -Visible -Type ([System.Windows.Automation.ControlType]::MenuItem)){throw 'Workspace context opened titlebar customization commands'}
    Capture 'customize-workspace-options' -Composed
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {!(Find ('workspace-switcher-show-'+$active)) -and (Control $id).Current.HasKeyboardFocus} 'Workspace context Escape did not restore editor focus'
    (Control $id).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
    Wait-Until {$null -ne (Find ('workspace-switcher-show-'+$active))} 'Editor keyboard context did not open visibility options'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {!(Find ('workspace-switcher-show-'+$active))} 'Editor keyboard context did not dismiss'
    if((HeaderJson) -ne $before -or (Model).windows_workspace.id -ne $active -or !(Model).header.editing){throw 'Workspace context changed the header or closed titlebar editing'}
    @{theme=$Theme;device=$Device;secondary_context='passed';escape_focus='passed';keyboard_context='passed';header_unchanged='passed'}|ConvertTo-Json|Set-Content (Join-Path $run 'workspace-options-input.json')
    if($Device -ne 'mouse'){
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        Wait-Until {(Gesture).phase -eq 'held' -and (Gesture).menu_open} 'Editor switcher hold did not open options'
        [CapyRowPointer]::Up()
        $null=Control ('workspace-switcher-show-'+$active)
        [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
        Wait-Until {!(Find ('workspace-switcher-show-'+$active))} 'Editor hold menu did not dismiss'
    }
    if((HeaderJson) -ne $before -or (Model).windows_workspace.id -ne $active -or !(Model).header.editing){throw 'Workspace context changed the header or closed titlebar editing'}
}
function Drop-Component([string]$Kind='space',[switch]$Cancel,[switch]$Picker) {
    $before=HeaderJson;$from=At ('header-component-'+$Kind)
    $zone=(Presentation).geometry.zones[1]
    $to=Screen @{x=$zone.x+$zone.width*.5;y=$zone.y+$zone.height*.5}
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    [CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {(Gesture).phase -eq 'dragging' -and $null -ne (Gesture).preview.target} 'Immediate component drag has no shared target'
    if((HeaderJson) -ne $before){throw 'Drag preview changed the workspace header'}
    if($Cancel){[CapyRowPointer]::Key(0x1b)}
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Header capture did not retire'
    if($Cancel){if((HeaderJson) -ne $before){throw 'Escape committed the component'}}
    elseif($Picker){Wait-Until {(Model).picker} 'Tool drop did not open the existing picker';if((HeaderJson) -ne $before){throw 'Opening the picker saved a placeholder'}}
    else{Wait-Until {(HeaderJson) -ne $before} 'Valid component drop did not insert';Check-Geometry}
}


function Check-ClockCaption {
    $clock=Entries|Where-Object {$_.item.kind -eq 'clock'}|Select-Object -First 1
    if($clock){
        Remove-HeaderItem $clock.id
    }
    Wait-Until {!(Entries|Where-Object {$_.item.kind -eq 'clock'}) -and (Find 'header-component-clock' -Visible)} 'Deleted Clock did not return to the component bank'
    $bank=Control 'header-component-clock' -Arranged
    $text=Control $bank.Current.Name -Name -Within $bank -Type ([System.Windows.Automation.ControlType]::Text) -Arranged
    $referenceName=$text.Current.Name;$referenceBounds=$text.Current.BoundingRectangle;$bankBounds=$bank.Current.BoundingRectangle
    $evidence=@{dpi=[CapyRowPointer]::GetDpiForWindow($drawingWindow);reference=@{name=$referenceName;bounds=$referenceBounds.ToString();width_pixels=$referenceBounds.Width;bank_bounds=$bankBounds.ToString();contained=$bankBounds.Contains($referenceBounds)}}
    $evidence|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'clock-caption.json')
    if(!$bankBounds.Contains($referenceBounds)){throw 'Clock component caption is not contained in its bank tile'}
    Drop-Component -Kind clock
    $clock=Entries|Where-Object {$_.item.kind -eq 'clock'}|Select-Object -First 1
    $placed=Control 'system-clock' -Type ([System.Windows.Automation.ControlType]::Text) -Arranged
    $bounds=$placed.Current.BoundingRectangle;$frame=(Control ('header-item-'+$clock.id) -Arranged).Current.BoundingRectangle
    $evidence.placed=@{name=$placed.Current.Name;bounds=$bounds.ToString();width_pixels=$bounds.Width;item_bounds=$frame.ToString();contained=$frame.Contains($bounds)}
    $evidence.width_shortfall_pixels=$referenceBounds.Width-$bounds.Width
    $evidence|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'clock-caption.json')
    if($placed.Current.Name -ne $referenceName -or !$frame.Contains($bounds) -or $bounds.Width -lt $referenceBounds.Width-1){throw "Placed Clock caption does not fit its native bank reference: $($bounds.Width)px versus $($referenceBounds.Width)px"}
}
function Check-ItemDrag([int]$Id) {
    $before=HeaderJson
    $box=(Presentation).geometry.items|Where-Object id -eq $Id|Select-Object -ExpandProperty bounds
    $from=At ('header-select-'+$Id)
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $distance=[int][Math]::Round($box.height*3*$scale)
    $far=@{x=$from.x;y=$from.y+$distance}
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    Wait-Until {(Gesture).phase -eq 'pressed' -and (Gesture).source.value -eq $Id} 'Header item did not receive the press'
    Start-Sleep -Milliseconds 900
    if($Device -eq 'mouse'){
        if((Gesture).menu_open -or (Gesture).phase -ne 'pressed'){throw 'Mouse hold opened a menu or started dragging'}
    }else{Wait-Until {(Gesture).phase -eq 'held' -and (Gesture).menu_open} 'Pen/touch hold did not open the item menu'}
    [CapyRowPointer]::Move($far.x,$far.y)
    Wait-Until {(Gesture).phase -eq 'dragging' -and (Gesture).preview.detached -and !(Gesture).menu_open} 'Held item did not detach and close its menu'
    $held=(Gesture).preview.held
    if([Math]::Abs($held.x-$box.x) -gt 1 -or [Math]::Abs($held.y-($box.y+$distance/$scale)) -gt 1){throw 'Detached item lost its original grab offset'}
    if((HeaderJson) -ne $before){throw 'Detached preview changed the workspace'}
    [CapyRowPointer]::Move($from.x,$from.y)
    Wait-Until {(Gesture).preview.target -and !(Gesture).preview.detached} 'Returning item did not reattach'
    [CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Item drag retained capture after Escape'
    if((HeaderJson) -ne $before){throw 'Cancelled item drag changed the header'}
    # Select through native keyboard activation before testing keyboard moves.
    Invoke ('header-select-'+$Id)
    foreach($step in 1..2){
        $previous=HeaderJson;[CapyRowPointer]::Key(0x27)
        Wait-Until {(HeaderJson) -ne $previous} 'Right key did not move the selected item'
    }
    if(!(@((Header).zones[2]|Where-Object id -eq $Id).Count)){throw 'Keyboard movement did not cross the zone boundary'}
}
function Check-ResizeCancel {
    $before=HeaderJson;$from=At 'header-component-space'
    $zone=(Presentation).geometry.zones[1]
    $to=Screen @{x=$zone.x+$zone.width*.5;y=$zone.y+$zone.height*.5}
    [CapyRowPointer]::Down($Device,$from.x,$from.y);[CapyRowPointer]::Move($to.x,$to.y)
    Wait-Until {(Gesture).preview.target} 'Resize case has no active placement preview'
    $size=$root.Current.BoundingRectangle
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]$size.Width+30) -Height ([int]$size.Height)
    Wait-Until {(Gesture).phase -eq 'idle'} 'Window resize did not cancel header capture'
    [CapyRowPointer]::Cancel()
    if((HeaderJson) -ne $before){throw 'Window resize committed a drag'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]$size.Width) -Height ([int]$size.Height)
    Check-Geometry
}
function Check-Fullscreen {
    foreach($active in @($true,$false)){
        $at=At 'header-item-1'
        [CapyRowPointer]::KeyAt(0x7a,$at.x,$at.y)
        Wait-Until {(Model).state.fullscreen -eq $active} 'Native F11 did not report the actual fullscreen state'
        Check-Geometry
        $clock=Entries|Where-Object {$_.item.kind -eq 'clock'}|Select-Object -First 1
        Wait-Until {(@((Presentation).geometry.items|Where-Object id -eq $clock.id).Count -gt 0) -eq $active} 'Clock did not follow fullscreen visibility'
        if((Model).header.editing){throw 'Fullscreen opened titlebar editing'}
    }
}

function Entries { @((Header).zones|ForEach-Object {$_}) }
function Picker-Search([string]$Query) {
    (Control 'tool-picker-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Query)
    Wait-Until {(Model).picker.query -eq $Query} 'Tool search did not reach shared state'
}
function Picker-Button([string]$Name) {
    (Control $Name -Name -Within (Control 'tool-picker') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Add-Tools {
    $before=HeaderJson;$firstId=(Header).next_id
    Drop-Component -Kind tools -Picker
    Picker-Button 'Cancel'
    Wait-Until {!((Model).picker) -and (Model).header.editing -and !(Find 'tool-picker') -and (Control 'drawing-canvas').Current.IsEnabled} 'Picker cancellation left the titlebar editor'
    if((HeaderJson) -ne $before){throw 'Picker Cancel changed the header'}
    Drop-Component -Kind tools -Picker
    foreach($tool in @(@('choose current paint color','picker-choice-color-0'),@('Brush','picker-choice-command-brush'))){
        Picker-Search $tool[0]
        (Control $tool[1]).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    }
    Picker-Search 'no-such-capy-tool-92831'
    Wait-Until {!((Model).picker.choices.Count)} 'Empty tool search retained choices'
    Picker-Search ''
    Wait-Until {(Model).picker.can_confirm} 'Search lost tool selection'
    Picker-Button 'Add Tools'
    Wait-Until {!((Model).picker) -and (Header).next_id -eq $firstId+2 -and !(Find 'tool-picker') -and (Control 'drawing-canvas').Current.IsEnabled} 'Picker did not add exactly two tools'
    $added=@(Entries|Where-Object id -ge $firstId)
    if($added[0].item.control.kind -ne 'color' -or $added[1].item.control.command -ne 'brush'){throw 'Tool insertion order changed across search'}
    Check-Geometry
    return $added
}
function Check-Drawer($Tools) {
    foreach($tool in $Tools){
        $frame=Control ('header-item-'+$tool.id)
        $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)
        $button=$frame.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
        $outer=$frame.Current.BoundingRectangle;$inner=$button.Current.BoundingRectangle
        if([Math]::Abs($outer.Width-$inner.Width) -gt 1 -or [Math]::Abs($outer.Height-$inner.Height) -gt 1){throw 'A native header tool does not fill its tile hit area'}
    }
    $color=$Tools[0].id;$brush=$Tools[1].id
    foreach($id in @($color,$brush,$brush)){
        $wasOpen=(Model).state.customization.drawer.anchor.id -eq $id
        $at=At ('header-item-'+$id)
        [CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
        if($wasOpen){
            Wait-Until {!((Model).state.customization.drawer)} 'Current header tool did not toggle its drawer closed'
        }else{
            Wait-Until {(Model).state.customization.drawer.anchor.kind -eq 'header' -and (Model).state.customization.drawer.anchor.id -eq $id} 'Header tool did not open or switch its drawer'
            $null=Control 'tool-drawer'
        }
    }
}


function Select-HeaderItem([int]$Id) {
    $route=@{zone=-1;direct=$false}
    Wait-Until {
        $entry=Find ('header-select-'+$Id) -Visible
        if($entry -and $entry.Current.IsEnabled -and !$entry.Current.BoundingRectangle.IsEmpty -and $entry.Current.BoundingRectangle.Width -gt 0 -and $entry.Current.BoundingRectangle.Height -gt 0){$route.direct=$true;return $true}
        $p=Presentation
        foreach($zone in 0..2){
            if($Id -notin $p.geometry.hidden[$zone]){continue}
            $overflow=Find ('header-overflow-'+$zone) -Visible
            if($overflow -and $overflow.Current.IsEnabled -and !$overflow.Current.BoundingRectangle.IsEmpty -and $overflow.Current.BoundingRectangle.Width -gt 0 -and $overflow.Current.BoundingRectangle.Height -gt 0){$route.zone=$zone;return $true}
        }
        $false
    } "Header item $Id has no visible selection route"
    if($route.direct){Invoke ('header-select-'+$Id);return}
    Invoke ('header-overflow-'+$route.zone);Invoke ('header-overflow-item-'+$Id)
    Wait-Until {!(Find ('header-overflow-item-'+$Id))} 'Hidden item selection left its menu open'
}
function Remove-HeaderItem([int]$Id) {
    Select-HeaderItem $Id
    $at=At 'header-edit-done';[CapyRowPointer]::KeyAt(0x2e,$at.x,$at.y)
    Wait-Until {!(@(Entries|Where-Object id -eq $Id).Count)} "Header item $Id was not removed"
}
function Check-Catalog {
    $initial=HeaderJson
    Edit-Header
    foreach($component in (Model).header.components){
        $kind=$component.item.kind
        if($kind -eq 'fullscreen'){throw 'Web-only fullscreen appeared in the native component bank'}
        $existing=@(Entries|Where-Object {$_.item.kind -eq $kind}).Count -gt 0
        if($component.singleton -and $existing){
            if(Find ('header-component-'+$kind)){throw "Present singleton $kind remained in the bank"}
        }else{
            Drop-Component -Kind $kind
            if($component.singleton -and (Find ('header-component-'+$kind))){throw "Added singleton $kind remained in the bank"}
        }
    }
    $settings=Entries|Where-Object {$_.item.kind -eq 'settings'}|Select-Object -First 1
    Remove-HeaderItem $settings.id
    $null=Control 'header-component-settings'
    Drop-Component -Kind settings
    Capture 'catalog'
    Invoke 'header-edit-cancel'
    Wait-Until {(HeaderJson) -eq $initial -and !(Model).header.editing} 'Catalog Cancel lost the starting layout'
    Edit-Header
    foreach($id in @(Entries|ForEach-Object {$_.id})){Remove-HeaderItem $id}
    Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing -and !(Entries).Count} 'Empty titlebar did not commit'
    $null=Control 'header-recovery-menu'
    WindowCommand 'undo_workspace'
    Wait-Until {(HeaderJson) -eq $initial} 'Recovery menu could not undo the empty titlebar'
    Check-Geometry
    & (Join-Path $PSScriptRoot 'exercise-header-settings.ps1') -ProcessId $review.Id -StateFile (State-File) -EvidenceDirectory $run
    Check-Geometry
}

function Check-Keyboard {
 $initial=HeaderJson
 (Control 'header-select-1').SetFocus()
 $at=At 'header-select-1';[CapyRowPointer]::KeyAt(0x20,$at.x,$at.y)
 Wait-Until {(Control 'header-select-1').Current.HasKeyboardFocus} 'Space moved editor focus'
 $seen=[Collections.Generic.List[object]]::new()
 for($i=0;$i -lt 55;$i++){
   [CapyRowPointer]::KeyAt(0x09,$at.x,$at.y);Start-Sleep -Milliseconds 70
   $focus=[System.Windows.Automation.AutomationElement]::FocusedElement
   if($focus.Current.ProcessId -ne $review.Id){throw 'Tab left the owned application'}
   $record=@{id=$focus.Current.AutomationId;name=$focus.Current.Name;parent=''}
   for($node=$focus;$node;$node=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($node)){
     if($node.Current.AutomationId -like 'header-item-*'){$record.parent=$node.Current.AutomationId;break}
     if($node.Current.NativeWindowHandle -eq $drawingWindow){break}
   }
   $seen.Add($record)
   if($record.parent -and $record.id -notlike 'header-select-*'){$seen|ConvertTo-Json|Set-Content (Join-Path $run 'tab-order.json');throw 'Tab focused an underlying active tool while editing'}
   if($record.id -eq 'header-edit-done'){break}
 }
 $seen|ConvertTo-Json|Set-Content (Join-Path $run 'tab-order.json')
 if($seen[-1].id -ne 'header-edit-done'){throw 'Tab could not reach Done'}
 if((HeaderJson) -ne $initial -or (Model).chrome_hidden){throw 'Keyboard selection activated a normal header tool'}

}
function Check-Narrow {
 $initial=HeaderJson
 $size=$root.Current.BoundingRectangle
 $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](640*$scale)) -Height ([int](480*$scale))
 Check-Geometry
 foreach($i in 1..12){Drop-Component}
 foreach($sizeName in @('small','medium','large')){
   Invoke ('header-size-'+$sizeName)
   Wait-Until {(Header).size -eq $sizeName} 'Narrow header size did not change'
   Check-Geometry
   $p=Presentation;$hidden=$null;$zoneIndex=0
   foreach($zoneIndex in 0..2){if($p.geometry.hidden[$zoneIndex].Count){$hidden=$p.geometry.hidden[$zoneIndex][0];break}}
   if(!$hidden){throw 'Fixture did not produce overflow at the minimum window size'}
   $ids=@(Entries|ForEach-Object {$_.id})
   Invoke ('header-overflow-'+$zoneIndex)
   Invoke ('header-overflow-item-'+$hidden)
   Wait-Until {!(Find ('header-overflow-item-'+$hidden))} 'Overflow menu did not close after selecting an item'
   $at=At ('header-overflow-'+$zoneIndex);[CapyRowPointer]::KeyAt(0x2e,$at.x,$at.y)
   Wait-Until {!(@(Entries|Where-Object id -eq $hidden).Count)} 'Delete did not remove the selected hidden item'
   $remaining=@(Entries|ForEach-Object {$_.id})
   if($remaining.Count -ne $ids.Count-1 -or @($ids|Where-Object {$_ -ne $hidden -and $_ -notin $remaining}).Count){throw 'Overflow deletion lost a hidden neighbor'}
   $done=Control 'header-edit-done';$bounds=$done.Current.BoundingRectangle;$window=$root.Current.BoundingRectangle
   if($done.Current.IsOffscreen -or $bounds.Left -lt $window.Left -or $bounds.Right -gt $window.Right -or $bounds.Bottom -gt $window.Bottom){throw 'Done is unreachable at the minimum window size'}
   Capture ('narrow-'+$sizeName)
 }
 Invoke 'header-edit-cancel'
 Wait-Until {!(Model).header.editing -and (HeaderJson) -eq $initial} 'Narrow overflow Cancel lost the starting layout'
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]$size.Width) -Height ([int]$size.Height)
 Check-Geometry
}

function Set-ZenTheme([string]$Theme) {
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $Theme -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq $Theme.ToLower()} 'Theme did not apply'
    Invoke 'CloseButton' -Within $dialog
    Wait-Until {!(Find 'Preferences' -Name) -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not release canvas'
}

function Use-ZenWorkspace([string]$Workspace) {
    $id='builtin:workspace:'+$Workspace
    if((Model).windows_workspace.id -ne $id){
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
        (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        $choice=@((Model).windows_workspace.switcher_display|Where-Object id -eq $id)[0]
        (Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    }
    Wait-Until {(Model).windows_workspace.id -eq $id -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Workspace did not switch'
    if($Workspace -eq 'photographer'){
        $null=Control 'histogram-source' -Arranged
        Wait-Until {$view=(Workspace-Root).Current.ItemStatus|ConvertFrom-Json;$chart=@($view.elements|Where-Object id -eq 'histogram-chart')[0];$chart.actual_bounds.width -gt 0 -and $chart.actual_bounds.height -gt 0} 'Photo workspace did not arrange its native histogram'
    }
}

function Check-ZenToolbar([string]$Theme) {
    function ToolbarGroup { @((Model).layout.groups|Where-Object active -eq 'toolbar')[0] }
    function Toggle-ZenToolbar {
        $grip=Control 'ribbon-grip-toolbar' -Arranged
        $grip.SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
        Invoke 'Show in Zen mode' -Name
        Wait-Until {!(Find 'Show in Zen mode' -Name -Visible)} 'Show in Zen mode menu did not close'
    }
    $group=ToolbarGroup
    if(!$group -or $group.floating -or $group.zen_kept){throw 'Zen toolbar fixture needs an ordinary hidden docked toolbar'}
    $grip=Control 'ribbon-grip-toolbar' -Arranged
    $identity=[string]::Join(',', $grip.GetRuntimeId())
    Toggle-ZenToolbar
    Wait-Until {(ToolbarGroup).zen_kept} 'Show in Zen mode did not update shared toolbar geometry'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)
    Wait-Until {(Model).chrome_hidden -and (Model).state.workspace.zen_mode -and (Find 'ribbon-grip-toolbar' -Visible)} 'Show in Zen mode hid the native toolbar'
    if([string]::Join(',', (Control 'ribbon-grip-toolbar').GetRuntimeId()) -ne $identity){throw 'Show in Zen mode replaced the retained native toolbar'}
    Capture ('zen-toolbar-kept-'+$Theme) -WithModel
    $otherTheme=if($Theme -eq 'Dark'){'Light'}else{'Dark'}
    foreach($choice in @($otherTheme,$Theme)){
        Set-ZenTheme $choice
        Wait-Until {(Model).chrome_hidden -and (Model).state.workspace.zen_mode -and (Find 'ribbon-grip-toolbar' -Visible)} 'Changing theme revealed chrome or hid the kept Zen toolbar'
    }
    Toggle-ZenToolbar
    Wait-Until {(Model).chrome_hidden -and !(ToolbarGroup).zen_kept -and !(Find 'ribbon-grip-toolbar' -Visible)} 'Unchecking Show in Zen mode kept the native toolbar visible'
    $y=$group.bounds.y+$group.bounds.height/2
    $start=Screen @{x=4;y=$y};$end=Screen @{x=84;y=$y}
    [CapyRowPointer]::Down('touch',$start.x,$start.y)
    try{for($step=1;$step -le 8;$step++){[CapyRowPointer]::Move([int]($start.x+($end.x-$start.x)*$step/8),$start.y);Start-Sleep -Milliseconds 20}}finally{[CapyRowPointer]::Up()}
    Wait-Until {(Model).chrome_hidden -and (ToolbarGroup).zen_kept -and (Find 'ribbon-grip-toolbar' -Visible)} 'Native edge touch swipe did not reveal the toolbar in Zen'
    Capture ('zen-toolbar-touch-peek-'+$Theme) -WithModel
    $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $x=[int]($canvas.Left+$canvas.Width*.6);$y=[int]($canvas.Top+$canvas.Height*.6)
    [CapyRowPointer]::Down('touch',$x,$y);[CapyRowPointer]::Up()
    Wait-Until {(Model).chrome_hidden -and !(ToolbarGroup).zen_kept -and !(Find 'ribbon-grip-toolbar' -Visible)} 'Outside touch did not hide the peeked toolbar'
    (Control 'drawing-canvas').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x4B)
    Wait-Until {(Find 'command-search' -Visible) -and (Control 'command-search').Current.HasKeyboardFocus -and (Model).state.workspace.zen_mode} 'Command search did not open in Zen'
    [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Find 'command-search' -Visible) -and !(Search-State) -and (Model).chrome_hidden -and (Model).state.workspace.zen_mode} 'One Escape did not close only command search in Zen'
    (Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {!(Model).chrome_hidden -and !(Model).state.workspace.zen_mode -and (Find 'ribbon-grip-toolbar' -Visible)} 'Escape did not leave Zen and restore ordinary chrome'
    if((Model).state.document_file.modified){throw 'Zen toolbar and edge swipe edited the drawing'}
    Write-Output "PASS $Theme Show in Zen mode toggle, retained toolbar, theme changes, native edge touch peek, outside dismissal and single-action Escape"
}

function Check-ZenMatrix {
    function CapyImage($Button) {
        $image=$Button.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Image))
        if(!$image){throw 'Native Capy image bounds are unavailable'}
        $image
    }
    function ImageBounds($Button) { (CapyImage $Button).Current.BoundingRectangle }
    function CapyIdentity($Button) { [string]::Join(',', $Button.GetRuntimeId()) + ':' + [string]::Join(',', (CapyImage $Button).GetRuntimeId()) }
    function SameBounds($Before,$After,[string]$Label) {
        foreach($field in @('Left','Top','Width','Height')) {
            if([Math]::Abs($Before.$field-$After.$field) -gt 1){throw "$Label $field differs: $($Before.$field) / $($After.$field)"}
        }
    }
    function Check-CapyBounds([string]$Label,[switch]$Fallback) {
        $normalControl=$null
        if($Fallback){Wait-Until {!(Find 'zen-capy' -Visible)} 'Fallback retained a normal Capy'}
        else {
            $normalControl=Control 'zen-capy' -Arranged
            $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
            $tile=@{small=36;medium=48;large=60}[(Header).size]*$scale
            if([Math]::Abs($normalControl.Current.BoundingRectangle.Width-$tile) -gt 1){throw "$Label normal Capy size differs"}
        }
        $normal=if($normalControl){$normalControl.Current.BoundingRectangle}else{$fallbackNormal}
        $glyph=if($normalControl){ImageBounds $normalControl}else{$fallbackGlyph}
        $identity=if($normalControl){CapyIdentity $normalControl}else{$null}
        $layout=(Model).layout|ConvertTo-Json -Compress -Depth 70
        $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
        Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $drawingWindow} 'Native review did not regain foreground'
        if($normalControl){[CapyRowPointer]::Down('mouse',[int]($normal.Left+$normal.Width/2),[int]($normal.Top+$normal.Height/2));[CapyRowPointer]::Up()}
        else {(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x09)}
        Wait-Until {(Model).chrome_hidden} 'Zen did not activate'
        if($identity -and (CapyIdentity (Control 'zen-capy')) -ne $identity){throw "$Label replaced native Capy button/image on entry"}
        SameBounds $normal (Control 'zen-capy').Current.BoundingRectangle "$Label button"
        SameBounds $glyph (ImageBounds (Control 'zen-capy')) "$Label glyph"
        Capture ('zen-bounds-'+($Label -replace '/','-'))
        $at=(Control 'zen-capy').Current.BoundingRectangle
        [CapyRowPointer]::Down('mouse',[int]($at.Left+$at.Width/2),[int]($at.Top+$at.Height/2));[CapyRowPointer]::Up()
        Wait-Until {!(Model).chrome_hidden -and !(Model).state.workspace.zen_mode} 'Capy did not leave Zen'
        if($normalControl){
            if((CapyIdentity (Control 'zen-capy')) -ne $identity){throw "$Label replaced native Capy button/image on exit"}
            SameBounds $normal (Control 'zen-capy').Current.BoundingRectangle "$Label restored button"
            SameBounds $glyph (ImageBounds (Control 'zen-capy')) "$Label restored glyph"
        }elseif(Find 'zen-capy' -Visible){throw "$Label revealed a hidden normal Capy"}
        if(((Model).layout|ConvertTo-Json -Compress -Depth 70) -ne $layout){throw "$Label changed workspace geometry"}
        Write-Output "PASS $Label native button/glyph bounds and restored layout"
    }
    foreach($theme in @('Dark','Light')) {
        Set-ZenTheme $theme
        foreach($workspace in @('illustrator','painter','photographer')) {
            Use-ZenWorkspace $workspace
            if($workspace -eq 'illustrator'){Check-ZenToolbar $theme}
            foreach($size in @('small','medium','large')) {
                Edit-Header
                Invoke ('header-size-'+$size)
                Wait-Until {(Header).size -eq $size} 'Header size did not update'
                Invoke 'header-edit-done'
                Wait-Until {!(Model).header.editing} 'Header editor did not close'
                Start-Sleep -Milliseconds 300
                Check-CapyBounds "$workspace/$theme/$size"
            }
        }
    }
    Edit-Header
    Invoke 'header-size-small';Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing -and (Header).size -eq 'small'} 'Small header did not settle'
    Start-Sleep -Milliseconds 300
    $fallbackNormal=(Control 'zen-capy').Current.BoundingRectangle
    $fallbackGlyph=ImageBounds (Control 'zen-capy')
    $capy=@(Entries|Where-Object {$_.item.kind -eq 'capy'})[0].id
    foreach($zone in @(1,2)){
        Edit-Header;Select-HeaderItem $capy
        for($attempt=0;$capy -notin (Header).zones[$zone].id -and $attempt -lt 128;$attempt++){$before=HeaderJson;[CapyRowPointer]::Key(0x27);Wait-Until {(HeaderJson) -ne $before} 'Native right key did not move Capy'}
        if($capy -notin (Header).zones[$zone].id){throw 'Capy did not reach the requested zone'}
        Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Header editor did not close'
        Start-Sleep -Milliseconds 300
        $null=Control 'zen-capy' -Arranged
        Check-CapyBounds ('custom-zone-'+$zone)
    }
    Edit-Header;Select-HeaderItem $capy
    for($attempt=0;(Header).zones[2][-1].id -ne $capy -and $attempt -lt 128;$attempt++){$before=HeaderJson;[CapyRowPointer]::Key(0x27);Wait-Until {(HeaderJson) -ne $before} 'Native right key did not move Capy'}
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Overflow header editor did not close'
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](640*$scale)) -Height ([int](500*$scale))
    Wait-Until {!(Find 'zen-capy' -Visible)} 'Narrow fixture did not overflow Capy'
    Check-CapyBounds 'custom-narrow-overflow' -Fallback
    Edit-Header;Remove-HeaderItem $capy
    Invoke 'header-edit-done';Wait-Until {!(Model).header.editing} 'Removed-Capy header editor did not close'
    Check-CapyBounds 'custom-removed' -Fallback

}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme}|ConvertTo-Json))
    Start-Review 'initial';Capture 'normal'
    if($catalogJourney){
        Check-CompactWorkspace;Check-PenOrder;Check-Catalog
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;journey=$Journey;workspace_caption_padding_and_group='passed';paint_categories='passed';pen_preset_order_and_selection='passed';pen_shortcut_tooltip='passed';header_component_catalog='passed';run=$run}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
        Get-Content (Join-Path $run 'results.json')
        return
    }
    Check-BackgroundContext
    if($Journey -eq 'background'){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;journey=$Journey;mouse_touch_pen_background_context='passed';run=$run}|ConvertTo-Json
        return
    }
    if($Journey -in @('zen','zen-workspace')){
        if($Journey -eq 'zen'){Check-ZenMatrix}
        else{foreach($theme in @('Dark','Light')){Set-ZenTheme $theme;foreach($workspaceKind in @('painter','photographer','illustrator')){Use-ZenWorkspace $workspaceKind};Check-ZenToolbar $theme}}
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{journey=$Journey;zen_native_button_and_glyph_bounds=if($Journey -eq 'zen'){'passed'}else{'outside scoped journey'};cases=if($Journey -eq 'zen'){22}else{0};zen_toolbar_visibility_and_retention='passed';zen_edge_touch_peek_and_outside_dismissal='passed';zen_retheme='passed';escape_search_then_zen='passed';toolbar_themes=@('dark','light')}|ConvertTo-Json
        return
    }
    if($Journey -in @('menus','menus-light')){
        Check-MenuHover 800;Check-MenuHover 480
        $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        foreach($width in @(620,1400)) {
            & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]($width*$scale)) -Height ([int](800*$scale))
            Check-Geometry
            if($width -eq 620){Check-MenuPreferences 'application-menus'}
        }
        (Control 'workspace-switch-builtin:workspace:painter').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:painter' -and !(Model).windows_workspace.busy} 'Sketch did not open'
        Check-Geometry
        Check-MenuPreferences 'application-primary-menu'
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;application_menu_hover='passed';compact_preferences='passed';closed_hover='passed';escape='passed';scope='OS-injected mouse on the owned desktop'}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
        Get-Content (Join-Path $run 'results.json')
        return
    }
    if($Journey -ne 'full'){
        $initial=HeaderJson
        Edit-Header;Check-WorkspaceOptions
        $entry=@(Entries|Where-Object {$_.item.kind -eq 'workspaces'})[0]
        if(!$entry){throw 'Default header does not contain a workspace selector'}
        Check-ItemDrag $entry.id
        Invoke 'header-edit-cancel'
        Wait-Until {!(Model).header.editing -and (HeaderJson) -eq $initial} 'Options journey changed the titlebar layout'
        Capture 'options-normal' -Composed
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        @{theme=$Theme;device=$Device;workspace_options_context='passed';workspace_placement_cancel='passed';keyboard='passed'}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
        Get-Content (Join-Path $run 'results.json')
        return
    }
    if($Workspace -eq 'paint'){Check-CompactWorkspace;Check-PenOrder}
    if($Catalog){Check-Catalog}
    $initial=HeaderJson;$footer=(Model).state.workspace.layout.canvas_info.visible
    Edit-Header;Check-WorkspaceOptions;Check-Keyboard
    foreach($size in @(@('medium',64),@('large',80),@('small',48))){
        Invoke ('header-size-'+$size[0])
        Wait-Until {(Header).size -eq $size[0] -and (Presentation).height -eq $size[1]} 'Size did not update the native header'
        Check-Geometry
    }
    $before=HeaderJson;$at=At 'header-component-space'
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
    Start-Sleep -Milliseconds 900
    if((Gesture).menu_open){throw 'A component-bank hold opened a menu'}
    [CapyRowPointer]::Up()
    if((HeaderJson) -ne $before){throw 'A component-bank click added an item'}
    Drop-Component -Cancel
    Check-ResizeCancel
    Drop-Component
    $space=@((Header).zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'space'})|Select-Object -Last 1
    if(!$space){throw 'No inserted space item'}
    Check-ItemDrag $space.id
    Remove-HeaderItem $space.id
    Wait-Until {(HeaderJson) -ne $before -and !(@((Header).zones|ForEach-Object {$_}|Where-Object id -eq $space.id).Count)} 'Delete did not remove selected header item'
    (Control 'header-show-footer').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).state.workspace.layout.canvas_info.visible -ne $footer} 'Footer preview did not change'
    Invoke 'header-edit-cancel'
    Wait-Until {!(Model).header.editing -and (HeaderJson) -eq $initial} 'Cancel did not restore the initial header'
    if((Model).state.workspace.layout.canvas_info.visible -ne $footer){throw 'Cancel changed footer visibility'}
    Edit-Header;Drop-Component;Check-ClockCaption;$tools=Add-Tools;Capture 'customized'
    Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing} 'Done did not close titlebar editing'
    $saved=HeaderJson
    Check-Geometry;Check-Drawer $tools;Check-Fullscreen
    WindowCommand 'undo_workspace'
    Wait-Until {(HeaderJson) -eq $initial} 'One workspace Undo did not restore the initial header'
    WindowCommand 'redo_workspace'
    Wait-Until {(HeaderJson) -eq $saved} 'One workspace Redo did not restore the edited header'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [CapyRowPointer]::Dispose()
    Start-Review 'restart'
    Wait-Until {(HeaderJson) -eq $saved -and !(Model).header.editing} 'Restart did not restore the completed header edit'
    Capture 'restarted'
    Edit-Header;Check-Narrow
    Edit-Header;Drop-Component
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    [CapyRowPointer]::Dispose()
    Start-Review 'unsaved-restart'
    Wait-Until {(HeaderJson) -eq $saved -and !(Model).header.editing} 'Closing without Done saved a titlebar preview'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
    [pscustomobject]@{theme=$Theme;workspace_options_context='passed';device=$Device;workspace=$Workspace;catalog=[bool]$Catalog;geometry='passed';sizes='passed';workspace_caption_padding_and_group='passed';workspace_sizing_visual_review='required';clock_caption_fit='passed';native_keyboard='passed';resize_cancel='passed';held_detach_reattach='passed';keyboard_zones='passed';footer_rollback='passed';fullscreen='passed';narrow_overflow='passed';unsaved_restart='passed';inert_bank='passed';immediate_component_drag='passed';preview_and_escape='passed';delete_and_cancel='passed';picker_search_and_order='passed';native_tile_hit_area='passed';drawer_switch_and_toggle='passed';done_undo_redo='passed';restart='passed';scope='OS-delivered synthetic input; physical devices and full visual acceptance are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){
        try{[CapyRowPointer]::Verify();@{gesture=Gesture;header=Header;presentation=Presentation;pointer_gap_ms=[CapyRowPointer]::MaxGapMilliseconds}|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run 'failure-state.json');Capture 'failure'}catch{}
        if($Device -eq 'mouse' -and [CapyRowPointer]::Active){try{[CapyRowPointer]::Key(0x1b)}catch{}}
    }
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
