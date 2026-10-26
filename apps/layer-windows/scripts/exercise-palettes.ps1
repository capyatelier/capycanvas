param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
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
$ControlType=[System.Windows.Automation.ControlType]
function Center($element){$b=$element.Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {$found.switch=Find ('workspace-switch-'+$Id);$found.menu=Find 'header-workspace-menu';$found.switch -or $found.menu} "No workspace switcher for $Name" 15
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
    @($items|Where-Object {$_.Current.ProcessId -eq $review.Id -and !$_.Current.IsOffscreen -and [System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($_).Current.ControlType -eq $ControlType::Menu})
}
function Close-Menu{
    Wait-Until {
        $focused=[System.Windows.Automation.AutomationElement]::FocusedElement
        $focused.Current.ProcessId -eq $review.Id -and $focused.Current.ControlType -eq $ControlType::MenuItem
    } 'Palette menu did not acquire keyboard focus'
    [CapyRowPointer]::Key([uint16]0x1B)
    Wait-Until {!@(Menu-Items).Count} 'Menu did not close'
}
function Menu-Invoke([string]$Id){
    $item=@{value=$null};Wait-Until {$item.value=@(Menu-Items)|Where-Object {$_.Current.AutomationId -eq $Id}|Select-Object -First 1;$item.value} "Menu lacks $Id"
    $item.value.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Palette-EntryMenu($Id,[string]$Name){
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    (Control 'palette-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Name)
    $row=Control ('palette-choice-'+$Id) -Arranged
    if($row.Current.ProcessId -ne $review.Id){throw 'Palette menu row belongs to another process'}
    $at=Center $row;[CapyRowPointer]::RightClick($at.x,$at.y)
}
function Palette-DialogChecks{
    $original=(Panel).palette;$ids=@((Panel).palettes.id)
    $document=(Model).state.document_file|ConvertTo-Json -Depth 20 -Compress
    $color=(Model).color_panel.definition|ConvertTo-Json -Depth 20 -Compress
    if(@((Panel).palettes|Where-Object {$_.name -in @('DG1 fixture palette','DG1 renamed palette')}).Count){throw 'Palette dialog fixture requires fresh isolated names'}
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    Invoke 'palette-library-add';Menu-Invoke 'palette-command-new_palette'
    $input=Control 'palette-name-input' -Arranged
    if($input.Current.ProcessId -ne $review.Id){throw 'New Palette input belongs to another process'}
    Capture 'palette-new-dialog' -WithModel -Composed
    $input.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('DG1 fixture palette')
    Wait-Until {(Control 'PrimaryButton').Current.IsEnabled} 'Valid isolated palette name did not enable Save'
    Invoke 'PrimaryButton'
    Wait-Until {!(Find 'palette-name-input') -and @((Panel).palettes|Where-Object name -eq 'DG1 fixture palette').Count -eq 1 -and !(Model).windows_palettes.busy} 'New Palette did not close and add exactly its named palette'
    $created=@((Panel).palettes|Where-Object name -eq 'DG1 fixture palette')[0].id
    if(@((Panel).palettes).Count -ne $ids.Count+1){throw 'New Palette changed more than one palette'}
    Palette-EntryMenu $created 'DG1 fixture palette';Menu-Invoke 'palette-command-rename_palette'
    $input=Control 'palette-name-input' -Arranged
    if($input.Current.ProcessId -ne $review.Id){throw 'Rename Palette input belongs to another process'}
    if($input.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -cne 'DG1 fixture palette'){throw 'Rename Palette lost the owned palette name'}
    Capture 'palette-rename-dialog' -WithModel -Composed
    $input.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('DG1 renamed palette')
    Wait-Until {(Control 'PrimaryButton').Current.IsEnabled} 'Palette rename did not enable Save'
    Invoke 'PrimaryButton'
    Wait-Until {!(Find 'palette-name-input') -and @((Panel).palettes|Where-Object id -eq $created)[0].name -ceq 'DG1 renamed palette' -and !(Model).windows_palettes.busy} 'Rename Palette did not preserve its exact identity and new name'
    $beforeCancel=[ordered]@{palettes=@(foreach($p in (Panel).palettes){[ordered]@{id=$p.id;name=$p.name}})}|ConvertTo-Json -Depth 8 -Compress
    Palette-EntryMenu $created 'DG1 renamed palette';Menu-Invoke 'palette-command-remove_palette'
    Control 'PrimaryButton' -Arranged|Out-Null
    Capture 'palette-remove-cancel-dialog' -WithModel -Composed
    Invoke 'CloseButton'
    Wait-Until {!(Find 'PrimaryButton' -Visible) -and (Control 'drawing-canvas').Current.IsEnabled} 'Remove Palette Cancel did not release its native dialog'
    $afterCancel=[ordered]@{palettes=@(foreach($p in (Panel).palettes){[ordered]@{id=$p.id;name=$p.name}})}|ConvertTo-Json -Depth 8 -Compress
    if($afterCancel -cne $beforeCancel){throw 'Remove Palette Cancel changed palette identities, names or order'}
    Palette-EntryMenu $created 'DG1 renamed palette';Menu-Invoke 'palette-command-remove_palette'
    Control 'PrimaryButton' -Arranged|Out-Null
    Capture 'palette-remove-confirm-dialog' -WithModel -Composed
    Invoke 'PrimaryButton'
    Wait-Until {!(Find 'PrimaryButton' -Visible) -and !@((Panel).palettes|Where-Object id -eq $created).Count -and !(Model).windows_palettes.busy} 'Remove Palette did not delete exactly its owned palette'
    if((@((Panel).palettes.id) -join ',') -cne ($ids -join ',')){throw 'Palette prompt workflow changed another palette or its order'}
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    (Control 'palette-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
    Invoke ('palette-choice-'+$original)
    Wait-Until {(Panel).palette -eq $original -and !(Find 'palette-search')} 'Palette prompt workflow did not restore the original active palette'
    if(((Model).state.document_file|ConvertTo-Json -Depth 20 -Compress) -cne $document -or ((Model).color_panel.definition|ConvertTo-Json -Depth 20 -Compress) -cne $color){throw 'Palette prompts changed artwork history or the current paint'}
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
function Drag($from,$to,[string]$Device='mouse',[switch]$Cancel,[switch]$Escape){
    $trace=[ordered]@{device=$Device;cancel=[bool]$Cancel;escape=[bool]$Escape;from=Input-Trace $from.x $from.y;to=Input-Trace $to.x $to.y;before=@(Swatches)}
    if($Cancel -or $Escape){$trace.color_before=(Model).color_panel.definition;$trace.selected_before=@(Swatches|Where-Object {(Control ('palette-swatch-'+$_)).Current.ItemStatus -eq 'Selected'})}
    $watch=[Diagnostics.Stopwatch]::StartNew()
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    try{for($i=1;$i -le 16;$i++){[CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$i/16),[int]($from.y+($to.y-$from.y)*$i/16));Start-Sleep -Milliseconds 16}
        Start-Sleep -Milliseconds 250
        $trace.release_ms=$watch.Elapsed.TotalMilliseconds;$trace.panel_before_release=(Control 'palettes-panel').Current.ItemStatus;$trace.preview_before_release=(Control 'palette-swatches').Current.ItemStatus
        if(($Cancel -or $Escape) -and ($trace.panel_before_release -ne 'Dragging' -or !$trace.preview_before_release)){throw "$Device cancellation did not start from a held drag preview"}
    }finally{
        if($Cancel){[CapyRowPointer]::Cancel()}
        else{if($Escape){[CapyRowPointer]::Key([uint16]0x1B)};[CapyRowPointer]::Up()}
        $trace.panel_after_release=(Control 'palettes-panel').Current.ItemStatus;$trace.preview_after_release=(Control 'palette-swatches').Current.ItemStatus;$trace.after=@(Swatches)
        if($Cancel -or $Escape){$trace.color_after=(Model).color_panel.definition;$trace.selected_after=@(Swatches|Where-Object {(Control ('palette-swatch-'+$_)).Current.ItemStatus -eq 'Selected'})}
        $trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ('palette-drag-'+$Device+'-'+[Guid]::NewGuid().ToString('N')+'.json'))
    }
}
function Palette-Scroller([string]$Id){
    $node=Control $Id -Arranged;$walker=[System.Windows.Automation.TreeWalker]::ControlViewWalker;$scroll=$null
    while($node -and !$node.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)){$node=$walker.GetParent($node)}
    if(!$scroll -or !$scroll.Current.VerticallyScrollable){throw "$Id did not expose an overflowing native scroll provider"}
    @{node=$node;pattern=$scroll}
}
function Palette-Scrollbars($Scroller){
    $viewport=$Scroller.node.Current.BoundingRectangle
    $condition=[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::ScrollBar)
    @($Scroller.node.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {
        $b=$_.Current.BoundingRectangle
        !$_.Current.IsOffscreen -and !$b.IsEmpty -and $b.Height -gt $b.Width -and $b.X -ge $viewport.Right-30 -and $b.X -le $viewport.Right+5 -and $b.Y -ge $viewport.Y-5 -and $b.Bottom -le $viewport.Bottom+5
    })
}
function Palette-ScrollState([string]$Id,$Scroller){
    $view=$Scroller.pattern.Current;$provider=$Scroller.node.Current.BoundingRectangle;$content=Native-Bounds $Id $scale
    [ordered]@{percent=$view.VerticalScrollPercent;view_size=$view.VerticalViewSize;horizontal_view_size=$view.HorizontalViewSize;provider=@($provider.X,$provider.Y,$provider.Width,$provider.Height);content=@($content.X,$content.Y,$content.Width,$content.Height);six_tiles=@(if($Id -eq 'palette-swatches'){foreach($id in (Swatches)[-6..-1]){$b=Native-Bounds ('palette-swatch-'+$id) $scale;@{id=$id;bounds=@($b.X,$b.Y,$b.Width,$b.Height)}}})}|ConvertTo-Json -Depth 8 -Compress
}
function Check-PaletteScrollModes([string]$Id){
    $scroller=Palette-Scroller $Id
    Scroll-Position (Control $Id -Arranged) 0
    $top=(Native-Bounds $Id $scale).Y
    Scroll-Position (Control $Id -Arranged) 50
    Wait-Until {(Native-Bounds $Id $scale).Y -lt $top} "$Id did not publish its actual scrolled content position"
    $panel=(Control 'palettes-panel').Current.BoundingRectangle
    $safe=@{x=[int]($panel.X+2);y=[int]($panel.Y+2)}
    if(!$panel.Contains([double]$safe.x,[double]$safe.y) -or $scroller.node.Current.BoundingRectangle.Contains([double]$safe.x,[double]$safe.y)){throw 'Scrollbar transition point is not inside owned panel padding and outside the scroller'}
    $beforeOrder=(Swatches) -join ',';$beforeColor=(Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress
    [CapyRowPointer]::Hover($safe.x,$safe.y)
    Wait-Until {@(Palette-Scrollbars $scroller).Count -eq 1} "$Id mouse mode did not show one native vertical scrollbar"
    $baseline=Palette-ScrollState $Id $scroller
    $tracePath=Join-Path $run ($Id+'-scroll-transitions.json')
    $records=[Collections.Generic.List[object]]::new();$records.Add(@{device='mouse';state=($baseline|ConvertFrom-Json)})
    $records|ConvertTo-Json -Depth 12|Set-Content $tracePath
    foreach($device in @('pen','touch')){
        if($device -eq 'pen'){[CapyRowPointer]::PenHover($safe.x,$safe.y)}else{[CapyRowPointer]::Down('touch',$safe.x,$safe.y)}
        try{
            Wait-Until {@(Palette-Scrollbars $scroller).Count -eq 0} "$Id $device mode retained a visible native scrollbar"
            $state=Palette-ScrollState $Id $scroller;$records.Add(@{device=$device;state=($state|ConvertFrom-Json)})
            $records|ConvertTo-Json -Depth 12|Set-Content $tracePath
            if($state -ne $baseline){throw "$Id $device mode changed exact scroll position, viewport or tile geometry"}
        }finally{if($device -eq 'pen'){[CapyRowPointer]::PenLeave()}else{[CapyRowPointer]::Up()}}
        [CapyRowPointer]::Hover($safe.x,$safe.y)
        Wait-Until {@(Palette-Scrollbars $scroller).Count -eq 1} "$Id mouse mode did not restore its native scrollbar after $device"
        $state=Palette-ScrollState $Id $scroller;$records.Add(@{device='mouse';after=$device;state=($state|ConvertFrom-Json)})
        $records|ConvertTo-Json -Depth 12|Set-Content $tracePath
        if($state -ne $baseline){throw "$Id mouse return changed exact scroll position, viewport or tile geometry"}
    }
    $bar=@(Palette-Scrollbars $scroller)[0]
    $thumbs=@($bar.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$ControlType::Thumb))|Where-Object {!$_.Current.IsOffscreen -and !$_.Current.BoundingRectangle.IsEmpty})
    if($thumbs.Count -ne 1){throw "$Id does not expose one arranged native scrollbar thumb"}
    $thumb=$thumbs[0].Current.BoundingRectangle;$track=$bar.Current.BoundingRectangle;$at=Center $thumbs[0]
    $to=@{x=$at.x;y=[int]($at.y+[Math]::Min($thumb.Height,$track.Bottom-$thumb.Bottom-1))}
    if($to.y -le $at.y -or !$track.Contains([double]$to.x,[double]$to.y)){throw "$Id thumb has no owned downward travel"}
    $percent=$scroller.pattern.Current.VerticalScrollPercent
    [CapyRowPointer]::Down('mouse',$at.x,$at.y)
    try{for($i=1;$i -le 8;$i++){[CapyRowPointer]::Move($at.x,[int]($at.y+($to.y-$at.y)*$i/8));Start-Sleep -Milliseconds 16}}finally{[CapyRowPointer]::Up()}
    Wait-Until {$scroller.pattern.Current.VerticalScrollPercent -gt $percent} "$Id mouse thumb drag did not move the scroll offset"
    if(((Swatches) -join ',') -ne $beforeOrder -or ((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -ne $beforeColor){throw "$Id scrollbar interaction changed swatches or the exact current color"}
    $records.Add(@{device='mouse-thumb';from=$at;to=$to;state=((Palette-ScrollState $Id $scroller)|ConvertFrom-Json)})
    $records|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ($Id+'-scroll-transitions.json'))
    Capture ($Id+'-mouse-scrollbar')
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned palettes review $($review.Id): $run"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Palettes review did not start' 90
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    foreach($workspace in @(@('Paint','builtin:workspace:illustrator'),@('Photo','builtin:workspace:photographer'))){
        Switch-Workspace $workspace[0] $workspace[1]
        $first=if($workspace[0] -eq 'Paint'){'color'}else{'histogram'}
        $next=if($workspace[0] -eq 'Paint'){'palettes'}else{'waveform'}
        $group=@((Model).layout.groups|Where-Object {$_.panels -contains $first})[0]
        $index=[Array]::IndexOf(@($group.panels),$first)
        if($index -lt 0 -or $group.panels[$index+1] -ne $next){throw "$($workspace[0]) does not place $next after $first"}
    }
    Switch-Workspace 'Paint' 'builtin:workspace:illustrator'
    if(@((Panel).palettes).Count -lt 10){throw 'Starter palettes were not installed'}
    Invoke 'panel-tab-palettes'
    $null=Control 'palettes-panel'
    Wait-Until {@(Swatches).Count -gt 5 -and (Control ('palette-swatch-'+(Swatches)[0])).Current.BoundingRectangle.Y -eq (Control ('palette-swatch-'+(Swatches)[5])).Current.BoundingRectangle.Y} 'Six saved swatches did not render across the first row'
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $panel=Native-Bounds 'palettes-panel' $scale
    $selector=Native-Bounds 'palette-selector' $scale
    $name=Native-Bounds 'palette-name' $scale
    $first=Native-Bounds ('palette-swatch-'+(Swatches)[0]) $scale
    @{theme=$Theme;dpi=96*$scale;panel=$panel;selector=$selector;name=$name;first_swatch=$first}|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'palette-geometry.json')
    if([Math]::Abs(($selector.Left-$panel.Left)/$scale-8) -gt 1 -or [Math]::Abs(($first.Left-$panel.Left)/$scale-8) -gt 1 -or [Math]::Abs(($panel.Right-$name.Right)/$scale-8) -gt 1){throw 'Palette content does not use equal eight-pixel side insets'}
    if([Math]::Abs($selector.Height/$scale-26) -gt 1 -or [Math]::Abs($name.Height/$scale-26) -gt 1){throw 'Palette footer controls are not compact'}
    Capture 'paint-palettes'

    $count=@(Swatches).Count
    Invoke 'palette-add'
    Wait-Until {@(Swatches).Count -eq $count+1} 'The + tile did not save the current color'
    $added=(Swatches)[-1]
    Wait-Until {(Find ('palette-swatch-'+$added)).Current.ItemStatus -eq 'Selected'} 'The added swatch was not selected'
    Invoke 'palette-name'
    $editor=Control 'palette-name-editor' -Type $ControlType::Edit
    $editBounds=Native-Bounds 'palette-name-editor' $scale
    if([Math]::Abs($editBounds.Right-$name.Right) -gt 1 -or [Math]::Abs($editBounds.Height/$scale-26) -gt 1){throw 'Palette name editor does not retain the readout trailing edge and compact height'}
    Capture 'palette-name-editor'
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
        $moved=(Swatches) -join ','
        (Control ('palette-swatch-'+(Swatches)[0])).SetFocus()
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x5A)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $before} "Ctrl+Z did not undo the $device reorder"
        $a=Control ('palette-swatch-'+(Swatches)[0]);$b=Control ('palette-swatch-'+(Swatches)[2])
        $colorBefore=(Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress
        $selectedBefore=@(Swatches|Where-Object {(Control ('palette-swatch-'+$_)).Current.ItemStatus -eq 'Selected'}) -join ','
        Drag (Center $a) (Center $b) $device -Cancel:($device -ne 'mouse') -Escape:($device -eq 'mouse')
        Wait-Until {((Swatches) -join ',') -eq $before -and (Control 'palettes-panel').Current.ItemStatus -eq 'Ready'} "$device cancellation changed swatch order or left a drag active"
        if(((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -ne $colorBefore -or (@(Swatches|Where-Object {(Control ('palette-swatch-'+$_)).Current.ItemStatus -eq 'Selected'}) -join ',') -ne $selectedBefore){throw "$device cancellation activated a swatch or changed the exact color definition"}
        (Control ('palette-swatch-'+(Swatches)[0])).SetFocus()
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x59)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $moved} "$device cancellation discarded the seeded Redo"
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x5A)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $before} "Ctrl+Z did not restore the original order after $device cancellation and Redo"
        if($selectedBefore -notmatch '^\d+$'){throw 'Cancellation tap checks require one selected saved swatch'}
        $tapId=(Swatches)[0];$invokeId=(Swatches)[1]
        $tapColor=((Panel).swatches|Where-Object id -eq $tapId).color|ConvertTo-Json -Depth 12 -Compress
        Tap (Center (Control ('palette-swatch-'+$tapId))) $device
        Wait-Until {(Control ('palette-swatch-'+$tapId)).Current.ItemStatus -eq 'Selected' -and ((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -eq $tapColor} "$device short tap did not select a swatch after cancellation"
        $invokeColor=((Panel).swatches|Where-Object id -eq $invokeId).color|ConvertTo-Json -Depth 12 -Compress
        Invoke ('palette-swatch-'+$invokeId)
        Wait-Until {(Control ('palette-swatch-'+$invokeId)).Current.ItemStatus -eq 'Selected' -and ((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -eq $invokeColor} "Accessible swatch Invoke did not select a color after $device cancellation"
        Invoke ('palette-swatch-'+$selectedBefore)
        Wait-Until {(Control ('palette-swatch-'+$selectedBefore)).Current.ItemStatus -eq 'Selected' -and ((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -eq $colorBefore} "The original selected color was not restored after $device cancellation tap checks"
    }
    $before=(Swatches) -join ','
    $a=Control ('palette-swatch-'+(Swatches)[0])
    $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$area=(Model).state.camera.work_area
    $outside=@{x=[int]($canvas.X+$area[0]+$area[2]/2);y=[int]($canvas.Y+$area[1]+$area[3]/2)}
    $workArea=[System.Windows.Rect]::new($canvas.X+$area[0],$canvas.Y+$area[1],$area[2],$area[3])
    $grid=(Control 'palette-swatches' -Arranged).Current.BoundingRectangle
    if(!$canvas.Contains([double]$outside.x,[double]$outside.y) -or !$workArea.Contains([double]$outside.x,[double]$outside.y) -or $grid.Contains([double]$outside.x,[double]$outside.y)){throw 'Outside-grid release point is not inside the owned canvas work area and outside the swatch grid'}
    Drag (Center $a) $outside
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
    Palette-DialogChecks

    $overflow=Join-Path $run 'scroll fixture.gpl'
    $gpl=@('GIMP Palette','Name: Scroll fixture','Columns: 6','#')
    for($i=0;$i -lt 36;$i++){$gpl+=('{0} {1} {2} Scroll {3:D2}' -f (($i*37)%256),(($i*71)%256),(($i*109)%256),$i)}
    [IO.File]::WriteAllLines($overflow,$gpl)
    $count=@((Panel).palettes).Count
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    Invoke 'palette-library-add';Menu-Invoke 'palette-command-import_palette';Picker 'Open' $overflow
    Wait-Until {@((Panel).palettes).Count -eq $count+1 -and !(Model).windows_palettes.busy} 'Overflow GPL import did not finish' 20
    if(!(Find 'palette-search')){Invoke 'palette-selector'}
    (Control 'palette-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
    $overflowPalette=@((Panel).palettes|Where-Object name -eq 'Scroll fixture')
    if($overflowPalette.Count -ne 1){throw 'Overflow import did not produce one isolated Scroll fixture palette'}
    Check-PaletteScrollModes 'palette-list'
    (Control 'palette-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Scroll fixture')
    Wait-Until {$choice=Find ('palette-choice-'+$overflowPalette[0].id);$choice -and !$choice.Current.IsOffscreen} 'Overflow palette did not arrange in its filtered chooser viewport'
    Invoke ('palette-choice-'+$overflowPalette[0].id)
    Wait-Until {(Panel).palette -eq $overflowPalette[0].id -and !(Find 'palette-search') -and @(Swatches).Count -eq 36} 'Overflow palette did not select and close its chooser'
    Check-PaletteScrollModes 'palette-swatches'
    foreach($device in @('mouse','pen','touch')){
        Scroll-Position (Control 'palette-swatches' -Arranged) 100
        $before=@(Swatches);$beforeOrder=$before -join ','
        $last=@(foreach($id in $before[-6..-1]){Native-Bounds ('palette-swatch-'+$id) $scale})
        $viewport=(Palette-Scroller 'palette-swatches').node.Current.BoundingRectangle
        for($i=0;$i -lt 6;$i++){
            if(!$viewport.Contains($last[$i]) -or $last[$i].Y -ne $last[0].Y -or $last[$i].Width -ne $last[0].Width -or $last[$i].Height -ne $last[0].Height -or ($i -gt 0 -and $last[$i].X -le $last[$i-1].Right)){throw 'Last palette row did not expose six equal, separated tiles inside its viewport'}
        }
        $id=$before[-1];$expectedColor=((Panel).swatches|Where-Object id -eq $id).color|ConvertTo-Json -Depth 12 -Compress
        Tap (Center (Control ('palette-swatch-'+$id))) $device
        Wait-Until {(Control ('palette-swatch-'+$id)).Current.ItemStatus -eq 'Selected' -and ((Model).color_panel.definition|ConvertTo-Json -Depth 12 -Compress) -eq $expectedColor} "$device last-tile hit did not select its exact color"
        Drag (Center (Control ('palette-swatch-'+$id))) (Center (Control ('palette-swatch-'+$before[-3]))) $device
        $expected=@($before[0..32])+@($before[35],$before[33],$before[34]);$moved=$expected -join ','
        Wait-Until {((Swatches) -join ',') -eq $moved} "$device last-tile drag did not reach its exact destination slot"
        (Control ('palette-swatch-'+$id)).SetFocus()
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x5A)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $beforeOrder} "$device last-tile drag did not undo in one step"
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x59)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $moved} "$device last-tile Redo did not restore exact order"
        [CapyRowPointer]::Hold(0x11,$true);try{[CapyRowPointer]::Key([uint16]0x5A)}finally{[CapyRowPointer]::Hold(0x11,$false)}
        Wait-Until {((Swatches) -join ',') -eq $beforeOrder} "$device last-tile final Undo did not restore the fixture"
    }
    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    $color=$null;foreach($zone in @((Model).header.model.zones)){foreach($entry in @($zone)){if($entry.item.control.kind -eq 'color'){$color=$entry.id}}}
    Tap (Center (Control ('header-item-'+$color)))
    Wait-Until {$d=(Model).state.customization.drawer;$d -and ((ConvertTo-Json -InputObject $d.columns -Compress) -eq '[["color","palettes"]]')} 'Sketch color drawer lacks Palettes below the wheel'
    Wait-Until {(Find 'drawer-panel-palettes') -and (Find 'palette-swatches' -Within (Find 'tool-drawer'))} 'Sketch drawer did not render Palettes'
    Capture 'sketch-drawer'
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
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
    Exit-CapyEnvironment
}
