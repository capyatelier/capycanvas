param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('all','diagnostics-fps','diagnostics-fps-light','panels','panels-light')][string]$Journey='all')
if($Journey.EndsWith('-light')){$Journey=$Journey.Substring(0,$Journey.Length-6);$Theme='light'}
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWorkspaceHeader {
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/workspace/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id}

function Panel-Menu([string]$Id){
    $window=(Model).application_menus|Where-Object id -eq 'window'
    foreach($section in $window.model.sections){foreach($item in $section){
        if($item.action.type -eq 'customize' -and $item.action.action.type -eq 'set_panel_visible' -and $item.action.action.panel -eq $Id){return $item}
    }}
    throw "Shared panel menu is missing: $Id"
}
function Drawing-Frames {
    $caption=Control 'Frames' -Name -Type ([System.Windows.Automation.ControlType]::Text)
    $reading=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetNextSibling($caption)
    if(!$reading -or $reading.Current.AutomationId -notlike 'renderer-stat-*'){throw 'Diagnostics Frames caption has no adjacent row reading'}
    [uint64]$reading.Current.Name
}
function Idle-Fps{
    Wait-Until {(Control 'renderer-stat-2').Current.Name -eq '0'} 'Diagnostics did not show zero FPS after canvas activity stopped' 15
    $frames=Drawing-Frames
    Start-Sleep -Milliseconds 1500
    if((Control 'renderer-stat-2').Current.Name -ne '0' -or (Drawing-Frames) -ne $frames){throw 'Idle Diagnostics created canvas drawing updates or reported active FPS'}
    $frames
}
function Diagnostics-Identity([string]$Stage){
    $cpu=Control 'renderer-stat-0' -Arranged;$rate=Control 'renderer-stat-2' -Arranged
    $identity=[ordered]@{cpu=($cpu.GetRuntimeId() -join ':');rate=($rate.GetRuntimeId() -join ':')}
    [ordered]@{stage=$Stage;generation=(Model).windows_gpu_generation;identity=$identity}|ConvertTo-Json -Depth 4 -Compress|Add-Content -LiteralPath (Join-Path $run 'diagnostics-identities.jsonl')
    $identity|ConvertTo-Json -Compress
}
function Assert-DiagnosticsIdentity([string]$Stage,[string]$Expected){
    if((Diagnostics-Identity $Stage) -cne $Expected){throw "$Stage replaced native metric rows"}
}
function Fps-Copy([string]$Tag){
    $catalog=Get-Content -LiteralPath (Join-Path $repo "assets/locales/$Tag/workspace.ftl") -Encoding UTF8
    foreach($key in 'diagnostics-submitted-fps','diagnostics-submitted-fps-description'){
        $line=@($catalog|Where-Object {$_.StartsWith($key+' = ')})
        if($line.Count -ne 1){throw "Missing shared Diagnostics copy: $Tag $key"}
        $line[0].Substring($key.Length+3).Trim()
    }
}
function Diagnostics-Language([string]$Tag){
    Invoke 'settings-button';Invoke 'preference-page-appearance'
    Wait-Until {Find 'preference-choice-language'} 'Language preference did not open'
    $state=State-File
    $bootstrap=Join-Path (Split-Path -Parent $state) ((Split-Path -Leaf $state) -replace '^ui-state-','bootstrap-')
    $index=[array]::IndexOf(@((Read-Snapshot $bootstrap).shipped_tags),$Tag)+1
    if($index -lt 1){throw "Missing registered language: $Tag"}
    $row=@((Model).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
    (Control 'preference-choice-language').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $row.kind.options[$index] -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).windows_active_tag -eq $Tag} "Language $Tag did not apply" 30
    Invoke 'CloseButton';Wait-Until {!(Model).preferences} 'Language preferences did not close'
    $copy=@(Fps-Copy $Tag)
    Wait-Until {(Control 'renderer-stat-2').Current.HelpText -eq $copy[1] -and (Find $copy[0] -Name -Type ([System.Windows.Automation.ControlType]::Text))} "Diagnostics retained stale source labels for $Tag"
}
function Preset-Workspace([string]$Name,[string]$Id){
    if((Model).windows_workspace.id -ne $Id){
        $entry=Find ('workspace-switch-'+$Id) -Visible
        if(!$entry){Invoke 'header-workspace-menu';$entry=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
        $entry.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    }
    Wait-Until {(Model).windows_workspace.id -eq $Id -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and !(Model).windows_workspace.saving -and (Model).brush_ready -and (Model).canvas_ready} "$Name workspace did not settle"
    if(Find 'workspace-manager' -Visible){(Control 'workspace-manager').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,27)}
    Wait-Until {!(Find 'workspace-manager' -Visible) -and (Control 'drawing-canvas').Current.IsEnabled} 'Workspace selector did not release the drawing'
}
function Preset-Groups([string]$Top){
    $groups=@((Model).layout.groups)
    $layers=@($groups|Where-Object {$_.panels -contains 'layers'})
    $properties=@($groups|Where-Object {$_.panels -contains 'properties'})
    $upper=@($groups|Where-Object {$_.panels -contains $Top})
    if($layers.Count -ne 1 -or $properties.Count -ne 1 -or $upper.Count -ne 1 -or $layers[0].panels -notcontains 'adjustments' -or $layers[0].id -eq $properties[0].id -or $layers[0].panels -contains 'properties'){throw 'Preset did not group Layers and Filters separately from Properties'}
    return @($upper[0],$properties[0],$layers[0])
}
function Preset-Geometry([string]$Top){
    $groups=@(Preset-Groups $Top);$records=@();$origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen([IntPtr]$root.Current.NativeWindowHandle,[ref]$origin)){throw 'Native client origin is unavailable'}
    foreach($group in $groups){
        $node=Control ('workspace-group-'+$group.id) -Arranged
        $group=@((Model).layout.groups|Where-Object id -eq $group.id)[0]
        if(!$group){throw 'Arranged preset group lost its current shared identity'}
        $box=Native-Bounds ([string]$group.id) $scale -Group;$shared=$group.bounds
        $receipt=[ordered]@{group=$group.id;shared=$shared;native_group=$box;uia_group=$node.Current.BoundingRectangle;client_origin=@($origin.x,$origin.y);control=@(Control-Trace $node)}
        try{
            if($node.Current.ProcessId -ne $review.Id -or !$root.Current.BoundingRectangle.Contains($box)){throw 'Preset group left its owned visible window'}
            foreach($pair in @(@($box.X,($origin.x+$shared.x*$scale)),@($box.Y,($origin.y+$shared.y*$scale)),@($box.Width,($shared.width*$scale)),@($box.Height,($shared.height*$scale)))){
                if([Math]::Abs($pair[0]-$pair[1]) -gt 1){throw 'Native preset allocation differs from the current shared group bounds'}
            }
            $body=Native-Bounds ('panel-body-scroll-'+$group.id) $scale
            $presentation=(Workspace-Root).Current.ItemStatus|ConvertFrom-Json
            $scroll=@($presentation.groups|Where-Object id -eq $group.id)[0].body_scroll
            $receipt.native_body=$body;$receipt.body_scroll=$scroll
            if(!$scroll -or !$box.Contains($body) -or !$root.Current.BoundingRectangle.Contains($body)){throw 'Preset body viewport left its owned group allocation'}
            $viewport=[double]$scroll.viewport_height;$extent=[double]$scroll.extent_height;$offset=[double]$scroll.vertical_offset
            foreach($value in @($viewport,$extent,$offset)){if([double]::IsNaN($value) -or [double]::IsInfinity($value)){throw 'Preset body scroll geometry is not finite'}}
            if($viewport -le 0 -or [Math]::Abs($body.Height-$viewport*$scale) -gt 1 -or $extent*$scale+1 -lt $group.min_body_height*$scale -or $offset -lt 0 -or $offset*$scale -gt [Math]::Max(0,$extent-$viewport)*$scale+1){throw 'Preset lost its finite scroll viewport or published minimum body extent'}
            $view=$null
            if(($extent-$viewport)*$scale -gt 1){
                $pattern=Scroll-Pattern $node
                $view=$pattern.Current.VerticalViewSize
                $receipt.provider=@{vertical=$pattern.Current.VerticallyScrollable;view_size=$view;percent=$pattern.Current.VerticalScrollPercent}
                if(!$pattern.Current.VerticallyScrollable -or $view -le 0 -or $view -gt 100 -or [double]::IsNaN($view) -or [Math]::Abs($extent*$scale*$view/100-$body.Height) -gt 1){throw 'Overflowing preset body has no matching native scroll provider'}
            }
        }catch{try{Capture 'preset-scroll-failure' -WithModel -Composed}catch{};throw}
        finally{$receipt|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ('preset-scroll-'+$group.id+'.json'))}
        $records+=@{id=$group.id;panels=$group.panels;shared=$shared;native=@($box.X,$box.Y,$box.Width,$box.Height);minimum_body=$group.min_body_height;native_body_height=$body.Height;native_extent_height=$extent*$scale;native_vertical_offset=$offset*$scale;native_vertical_view_size=$view}
    }
    if($records[0].native[1]+$records[0].native[3] -ge $records[1].native[1] -or $records[1].native[1]+$records[1].native[3] -ge $records[2].native[1]){throw 'Preset primary groups overlap or have the wrong vertical order'}
    return $records
}
function Check-PresetGroups{
    $initial=(Model).windows_workspace.id;$document=(Model).state.document_file|ConvertTo-Json -Depth 20 -Compress
    $records=@()
    foreach($preset in @(@{name='Paint';id='builtin:workspace:illustrator';top='navigator'},@{name='Photo';id='builtin:workspace:photographer';top='histogram'})){
        Preset-Workspace $preset.name $preset.id
        foreach($width in 1200,744){
            & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Resize -Width ([int]($width*$scale)) -Height ([int](700*$scale))
            $before=@(Preset-Geometry $preset.top);$layerGroup=@(Preset-Groups $preset.top)[2];$active=$layerGroup.active
            foreach($tab in 'adjustments','layers'){
                Invoke ('panel-tab-'+$tab)
                Wait-Until {@((Model).layout.groups|Where-Object id -eq $layerGroup.id)[0].active -eq $tab} 'Preset tab did not acknowledge its active panel'
                $after=@(Preset-Geometry $preset.top)
                for($i=0;$i -lt $before.Count;$i++){
                    if(($before[$i].shared|ConvertTo-Json -Compress) -cne ($after[$i].shared|ConvertTo-Json -Compress)){throw 'Switching Layers and Filters changed the shared group allocation'}
                    for($axis=0;$axis -lt 4;$axis++){if([Math]::Abs($before[$i].native[$axis]-$after[$i].native[$axis]) -gt 1){throw 'Switching Layers and Filters moved the native group frame'}}
                }
            }
            Invoke ('panel-tab-'+$active);Wait-Until {@((Model).layout.groups|Where-Object id -eq $layerGroup.id)[0].active -eq $active} 'Preset tab did not restore its original selection'
            $records+=@{workspace=$preset.id;width=$width;groups=$before;tab_bounds_retained=$true}
        }
        Invoke 'panel-tab-adjustments'
        Wait-Until {@((Model).layout.groups|Where-Object id -eq $layerGroup.id)[0].active -eq 'adjustments' -and !(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Filters tab did not finish autosaving'
        $saved=(Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress
        $other=if($preset.name -eq 'Paint'){@('Photo','builtin:workspace:photographer')}else{@('Paint','builtin:workspace:illustrator')}
        Preset-Workspace $other[0] $other[1];Preset-Workspace $preset.name $preset.id
        if(((Model).state.workspace.layout|ConvertTo-Json -Depth 80 -Compress) -cne $saved){throw 'Workspace reload lost its exact saved Filters tab and group layout'}
        $null=Preset-Geometry $preset.top
        Invoke ('panel-tab-'+$active)
        Wait-Until {@((Model).layout.groups|Where-Object id -eq $layerGroup.id)[0].active -eq $active -and !(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Reloaded preset did not restore and save its original active tab'
    }
    $names=@{'builtin:workspace:illustrator'='Paint';'builtin:workspace:photographer'='Photo';'builtin:workspace:painter'='Sketch'}
    if(!$names.ContainsKey($initial)){throw 'Preset fixture cannot restore an unknown initial workspace'}
    Preset-Workspace $names[$initial] $initial
    if(((Model).state.document_file|ConvertTo-Json -Depth 20 -Compress) -cne $document){throw 'Preset grouping review edited the document or history'}
    [IO.File]::WriteAllText((Join-Path $run 'preset-grouping.json'),($records|ConvertTo-Json -Depth 20),[Text.UTF8Encoding]::new($false))
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.Directory]::CreateDirectory($env:CAPY_STORAGE_DIR)|Out-Null
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/workspace-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned workspace review $($review.Id)"
    Wait-Until {$script:drawing=Owned-DrawingWindow $review;$null -ne $drawing -and (Model).brush_ready -and (Model).state.theme -eq $Theme} 'Review did not start' 45
    $root=$drawing.Root
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::SetForegroundWindow($drawing.Handle)|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $ids=@('file','edit','image','layer','select','filter','view','window')
    $scale=[CapyWorkspaceHeader]::GetDpiForWindow($drawing.Handle)/96.
    if($Journey -eq 'panels'){
        Check-PresetGroups
        Assert-RendererHealthy $review.Id $drawing
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
        [pscustomobject]@{theme=$Theme;journey=$Journey;preset_grouping_native_bounds_tabs_and_reload='passed';document_and_history='unchanged';zero_exit='passed';run=$run}|ConvertTo-Json
        return
    }
    if($Journey -eq 'all'){
    Check-PresetGroups
    foreach($size in @(@{width=744;compact=$true},@{width=1200;compact=$false},@{width=744;compact=$true})){
        $entry=& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File' -Inspect
        $entry.SetFocus();Wait-Until {$entry.Current.HasKeyboardFocus} 'Could not focus the visible menu entry'
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Resize -Width ([int]($size.width*$scale)) -Height ([int](700*$scale))
        $entryId=if($size.compact){'application-menus'}else{'application-menu-file'}
        Wait-Until {[System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq $entryId} 'Responsive header did not retain menu keyboard focus'
    }
    foreach($size in @(@{width=744;compact=$true},@{width=1200;compact=$false})){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Resize -Width ([int]($size.width*$scale)) -Height ([int](700*$scale))
        $entryId=if($size.compact){'application-menus'}else{'application-menu-file'}
        Wait-Until {$entry=Find $entryId -Type ([System.Windows.Automation.ControlType]::Button);$entry -and !$entry.Current.IsOffscreen} 'Header did not switch between compact and expanded menus'
        $last=-1
        foreach($id in $ids){
            $button=& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $id -PassThru
            $spec=(Model).application_menus|Where-Object id -eq $id
            if($button.Current.Name -ne $spec.label){throw "Menu label differs from shared model: $id"}
            $position=if($size.compact){$button.Current.BoundingRectangle.Y}else{$button.Current.BoundingRectangle.X}
            if($position -le $last){throw 'Application menus are not in shared order'}
            $last=$position
        }
    }
    Wait-Until {(Model).windows_workspace.ready -and !(Model).windows_workspace.busy -and (Command 'select_all').enabled} 'Document startup did not enable selection commands' 45
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'select'
    $all=Control 'select_all'
    if($all.Current.IsEnabled -ne (Command 'select_all').enabled){throw 'Selection menu enabled state differs from model'}
    Invoke 'select_all'
    Wait-Until {(Command 'deselect').enabled} 'Select All did not update shared selection'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'select';Invoke 'deselect'
    Wait-Until {!(Command 'deselect').enabled} 'Deselect did not clear selection'
    Invoke 'settings-button'
    Invoke 'preference-page-about'
    Wait-Until {$null -ne (Model).preferences -and (Model).preferences.page -eq 'about'} 'About did not open shared settings page'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    $about=(Model).preferences|ConvertTo-Json -Depth 40 -Compress
    if($about.IndexOf('"dedication"') -le $about.IndexOf('"source_code"') -or $about -notmatch 'Nagu'){throw 'About did not end with the dedication'}
    if(!$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Dedicated to'))){throw 'About did not show the dedication row'}
    $close=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Model).preferences -and (Control 'drawing-canvas').Current.IsEnabled} 'About did not close'
    }
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    $stats=Panel-Menu 'stats'
    # The current default keeps Diagnostics in an inactive Navigator tab.
    # Establish a hidden baseline before testing the menu's show/hide contract.
    if($stats.selected){
        (Control $stats.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {!(Panel-Menu 'stats').selected} 'Diagnostics did not leave the layout'
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    }
    (Control $stats.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {@((Model).layout.groups|Where-Object active -eq 'stats').Count -gt 0} 'Diagnostics panel did not appear'
    Wait-Until {$null -ne (Find 'renderer-stat-7')} 'Diagnostics query did not populate all shared rows'
    $cpu=Control 'renderer-stat-0'
    if($cpu.Current.HelpText -notmatch 'Not GPU completion or input latency'){throw 'Diagnostics lost measurement scope'}
    $framesBefore=Drawing-Frames
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
    Wait-Until {(Drawing-Frames) -gt $framesBefore} 'Visible Diagnostics did not collect drawing submissions'
    $copy=@(Fps-Copy 'en')
    Wait-Until {(Control 'renderer-stat-2').Current.HelpText -eq $copy[1] -and (Find $copy[0] -Name -Type ([System.Windows.Automation.ControlType]::Text))} 'Windows Diagnostics did not identify submitted canvas frames'
    $idleFrames=Idle-Fps
    $metricIdentity=Diagnostics-Identity 'after-stroke'
    $document=(Model).state.document_file|ConvertTo-Json -Depth 12 -Compress
    $cameraRevision=(Model).state.camera.revision
    $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    $area=(Model).state.camera.work_area;$viewport=(Model).state.camera.viewport;$factor=$bounds.Width/$viewport[0]
    $x=[int]($bounds.X+($area[0]+$area[2]*.5)*$factor);$y=[int]($bounds.Y+($area[1]+$area[3]*.5)*$factor)
    [CapyRowPointer]::SetForegroundWindow($drawing.Handle)|Out-Null
    [CapyRowPointer]::Down('mouse',$x,$y,'middle')
    try{for($step=1;$step -le 30;$step++){[CapyRowPointer]::Move(($x+[int](24*[Math]::Sin($step/5.))),$y);Start-Sleep -Milliseconds 20}}finally{[CapyRowPointer]::Up()}
    Wait-Until {(Model).state.camera.revision -gt $cameraRevision -and (Control 'renderer-stat-2').Current.Name -match '^[1-9][0-9]*$'} 'Cached native pan did not publish positive submitted FPS'
    if(((Model).state.document_file|ConvertTo-Json -Depth 12 -Compress) -ne $document){throw 'Native pan changed the document'}
    if((Idle-Fps) -lt $idleFrames){throw 'Native pan reset the cumulative drawing submissions'}
    Assert-DiagnosticsIdentity 'after-pan' $metricIdentity
    foreach($tag in 'ja','en'){
        Diagnostics-Language $tag
        Assert-DiagnosticsIdentity "after-language-$tag" $metricIdentity
    }
    Assert-DiagnosticsIdentity 'before-gpu-recovery' $metricIdentity
    $generation=(Model).windows_gpu_generation
    $camera=(Model).state.camera|ConvertTo-Json -Depth 12 -Compress
    $recovery=@{unavailable_observed=$false}
    Invoke 'Test GPU loss' -Name
    Wait-Until {
        $rate=Find 'renderer-stat-2';if($rate -and $rate.Current.Name -eq [string][char]0x2014){$recovery.unavailable_observed=$true}
        (Model).windows_gpu_generation -eq $generation+1 -and (Model).brush_ready
    } 'Diagnostics GPU recovery did not replace its renderer' 60
    $copy=@(Fps-Copy 'en')
    Wait-Until {(Control 'renderer-stat-2').Current.Name -match '^[0-9]+$' -and (Control 'renderer-stat-2').Current.HelpText -eq $copy[1] -and (Find $copy[0] -Name -Type ([System.Windows.Automation.ControlType]::Text))} 'Recovered Diagnostics did not resume submitted FPS'
    $null=Idle-Fps
    if(((Model).state.document_file|ConvertTo-Json -Depth 12 -Compress) -ne $document -or ((Model).state.camera|ConvertTo-Json -Depth 12 -Compress) -ne $camera){throw 'Diagnostics GPU recovery changed the document or camera'}
    Assert-DiagnosticsIdentity 'after-gpu-recovery' $metricIdentity
    [IO.File]::WriteAllText((Join-Path $run 'diagnostics-fps.json'),(@{cached_pan='passed';idle_without_drawing_updates='passed';retained_localized_source='passed';gpu_generation_before=$generation;gpu_generation_after=(Model).windows_gpu_generation;unavailable_observed=$recovery.unavailable_observed;unavailable_scope='200 ms native polling can miss the reset interval; windowless renderer_stats_distinguish_submissions_and_retire_surface_feedback asserts unavailable deterministically'}|ConvertTo-Json -Depth 4))
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Action Resize -Width 1500 -Height 1000
    Wait-Until {$null -ne (Find 'renderer-stat-7')} 'Resize removed Diagnostics content'
    Assert-RendererHealthy $review.Id $drawing
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -Output (Join-Path $run 'diagnostics.png') -ClientOnly *> (Join-Path $run 'diagnostics.json')
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control (Panel-Menu 'stats').label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {$null -eq (Find 'renderer-stat-0')} 'Hidden Diagnostics retained visible widgets'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'undo_workspace'
    Wait-Until {$null -ne (Find 'renderer-stat-7')} 'Workspace Undo did not restore Diagnostics'
    Assert-RendererHealthy $review.Id $drawing
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    if($Journey -eq 'diagnostics-fps'){
        [pscustomobject]@{journey=$Journey;diagnostics_query_and_row_retention='passed';diagnostics_visibility_and_workspace_undo='passed';native_resize='passed';zero_exit='passed';run=$run}|ConvertTo-Json
    }else{
    [pscustomobject]@{preset_grouping_native_bounds_tabs_and_reload='passed';shared_menus='passed';compact_and_expanded_header='passed';responsive_menu_focus='passed';selection_commands='passed';about_page='passed';diagnostics_query_and_row_retention='passed';diagnostics_visibility_and_workspace_undo='passed';native_resize='passed';zero_exit='passed';scope='native UI Automation; physical docking, drawers, full layout parity and presentation remain separate'}|ConvertTo-Json
    }
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose();Exit-CapyEnvironment
}
