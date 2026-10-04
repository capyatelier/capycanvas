param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
Add-Type -AssemblyName System.Drawing
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/expansion/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke $Id
}
function Toolbar([string]$Id){(Model).panels|Where-Object id -eq $Id}
function ToolbarContext([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function DialogButton([string]$Id,[string]$Name){
    $dialog=Control $Id;$found=@{item=$null}
    $buttonMatch=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
    Wait-Until {$found.item=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$buttonMatch);$null -ne $found.item} "Missing dialog button $Name"
    $found.item
}
function InvokeDialog([string]$Id,[string]$Name){(DialogButton $Id $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Configure([string]$Panel,[string]$Source="panel-tab-$Panel"){
    Wait-Until {$null -ne ((Model).panels|Where-Object id -eq $Panel)} "Missing shared panel $Panel"
    $model=(Model).panels|Where-Object id -eq $Panel
    ToolbarContext $Source
    Invoke ($model.configuration_title+'…') -Name
    Wait-Until {(Model).state.customization.expanded -eq $Panel} "Core did not expand $Panel"
    $configuration=Control "panel-configuration-$Panel"
    Wait-Until {$configuration.Current.BoundingRectangle.Width -gt 100} "Configuration $Panel has no native bounds"
    Start-Sleep -Milliseconds 400
    $configuration
}
function Dismiss([string]$Panel,[string]$Source="panel-tab-$Panel"){
    (Control $Source).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x1B)
    Wait-Until {$null -eq (Model).state.customization.expanded -and $null -eq (Find "panel-configuration-$Panel")} "Escape did not close $Panel configuration"
}
function Toggle([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
function Check-OverviewOverlap($Configuration){
    $overview=(Control 'navigator-overview' -Within $Configuration).Current.BoundingRectangle
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($review.MainWindowHandle,[ref]$origin)){throw 'Cannot locate client capture'}
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.
    $document=(Model).state.tabs[0]
    $fit=[Math]::Min(($overview.Width-8*$scale)/$document.width,($overview.Height-8*$scale)/$document.height)
    $width=$document.width*$fit;$height=$document.height*$fit
    $groups=@((Model).layout.groups|Where-Object {$_.active -ne 'navigator'})
    $hit=$null
    foreach($fy in 0.1,0.3,0.5,0.7,0.9){foreach($fx in 0.04,0.2,0.5,0.8,0.96){
        if($hit){break}
        $px=[int]($overview.Left-$origin.x+($overview.Width-$width)/2+$width*$fx)
        $py=[int]($overview.Top-$origin.y+($overview.Height-$height)/2+$height*$fy)
        foreach($group in $groups){$b=$group.bounds
            if($px -gt $b.x*$scale -and $px -lt ($b.x+$b.width)*$scale -and $py -gt ($b.y+36)*$scale -and $py -lt ($b.y+$b.height)*$scale){$hit=@{x=$px;y=$py};break}}
    }}
    if(!$hit){throw 'Fixture did not overlap a lower native panel'}
    $x=$hit.x;$y=$hit.y
    $before=[Drawing.Bitmap]::new((Join-Path $run 'navigator-before.png'))
    $after=[Drawing.Bitmap]::new((Join-Path $run 'navigator.png'))
    try{
        $old=$before.GetPixel($x,$y);$pixel=$after.GetPixel($x,$y)
        if($old.R -gt 245 -and $old.G -gt 245 -and $old.B -gt 245){throw 'Overlap reference was already white'}
        if($pixel.R -lt 245 -or $pixel.G -lt 245 -or $pixel.B -lt 245){throw 'Lower native panel covers the GPU overview'}
        return @{x=$x;y=$y;before=$old.ToArgb()}
    }finally{$before.Dispose();$after.Dispose()}
}
function Shown([string]$Panel,[string]$Control){((Model).panels|Where-Object id -eq $Panel).controls|Where-Object control -eq $Control|Select-Object -ExpandProperty visible_in_panel}
function Check-SizeGrid($Within,[string]$Device,[string]$Label){
    $found=@{tiles=@()}
    Wait-Until {
        $buttons=$Within.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new(
            [System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
        $found.tiles=@($buttons|Where-Object {$_.Current.AutomationId -like 'size-preset-*'})
        $found.tiles.Count -eq 40
    } "Size grid shows $($found.tiles.Count) of 40 presets"
    $tiles=$found.tiles
    $scale=[CapyRowPointer]::GetDpiForWindow($review.MainWindowHandle)/96.;$tile=@(36,44)
    $first=$tiles[0].Current.BoundingRectangle
    $row=@($tiles|Where-Object {[Math]::Abs($_.Current.BoundingRectangle.Top-$first.Top) -lt 1})
    foreach($item in $row){$box=$item.Current.BoundingRectangle
        if([Math]::Abs($box.Width-$tile[0]*$scale) -gt 1.5 -or [Math]::Abs($box.Height-$tile[1]*$scale) -gt 1.5){throw "Size tile $($item.Current.AutomationId) is not $($tile[0]) x $($tile[1]) DIP"}}
    if($row.Count -lt 6){throw "Size grid fits only $($row.Count) presets in a row"}
    $pitch=($row[1].Current.BoundingRectangle.Left-$first.Left)/$scale
    if([Math]::Abs($pitch-$tile[0]-2) -gt 1){throw "Size tiles are $pitch DIP apart"}
    $rows=[Math]::Ceiling($tiles.Count/$row.Count)
    $whole=[Math]::Abs($tiles[-1].Current.BoundingRectangle.Height-$tile[1]*$scale) -le 1.5
    if($whole -and [Math]::Abs(($tiles[-1].Current.BoundingRectangle.Bottom-$first.Top)/$scale-($rows*$tile[1]+($rows-1)*2)) -gt 2){throw 'Size grid rows do not wrap at the shared spacing'}
    $target=$tiles|Where-Object {$_.Current.AutomationId -eq "size-preset-$Label"}
    $value=[double]::Parse($Label,[Globalization.CultureInfo]::InvariantCulture)
    $box=$target.Current.BoundingRectangle;$x=[int]($box.Left+$box.Width/2);$y=[int]($box.Top+$box.Height/2)
    [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
    Wait-Until {[Math]::Abs((Model).state.brush.diameter-$value) -lt .001} "$Device tap on size $Label did not set the brush size"
    Wait-Until {$target.Current.ItemStatus -ne ''} "Size $Label did not show as selected"
    @{presets=$tiles.Count;columns=$row.Count;rows=$rows;wrap_checked=$whole;device=$Device;size=$Label}
}
try{
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/expansion-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned expansion review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null;[CapyRowPointer]::Initialize([uint32]$review.Id)
    if(Shown 'sizes' 'brush_size'){throw 'Sizes shows the Brush Size slider by default'}
    if((Model).state.customization.drawer -or !@((Model).layout.groups|Where-Object active -eq 'sizes').Count){Invoke-Id 'panel-tab-sizes'}
    $docked=Check-SizeGrid $root 'mouse' '1.5'
    $configuration=Configure 'sizes'
    $configured=@((Check-SizeGrid $configuration 'pen' '25'),(Check-SizeGrid $configuration 'touch' '0.7'))
    $identity=(Control 'configure-show-brush_size').GetRuntimeId() -join ':'
    $scale=(Control 'panel-tab-sizes').Current.BoundingRectangle.Height/36
    if([Math]::Abs($configuration.Current.BoundingRectangle.Width-380*$scale) -gt 2){throw 'Configuration does not use the shared 380 DIP width'}
    $before=(Model).state.brush.diameter
    (Control 'configure-brush_size-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.35)
    Wait-Until {(Model).state.brush.diameter -ne $before} 'Configuration slider did not update shared brush size'
    Toggle 'configure-show-brush_size'
    Wait-Until {Shown 'sizes' 'brush_size'} 'Visibility checkbox did not show the main brush size control'
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Visibility change replaced configuration controls'}
    Toggle 'configure-show-brush_size'
    Wait-Until {!(Shown 'sizes' 'brush_size')} 'Visibility checkbox did not hide the main brush size control again'
    Capture 'sizes'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Wait-Until {(Model).state.customization.expanded -eq 'sizes'} 'Resize dismissed configuration'
    Start-Sleep -Milliseconds 400
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Resize replaced configuration controls'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 720
    Start-Sleep -Milliseconds 400
    $placement=$configuration.Current.ItemStatus|ConvertFrom-Json
    if(!$placement -or $placement.configuration.width -gt 380){throw 'Narrow expansion lost shared geometry'}
    if(((Control 'configure-show-brush_size').GetRuntimeId() -join ':') -ne $identity){throw 'Narrow resize replaced configuration controls'}
    Capture 'sizes-narrow'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1500 -Height 1000
    Start-Sleep -Milliseconds 400
    Dismiss 'sizes'
    $configuration=Configure 'layers' $(if(Find 'panel-tab-layers'){'panel-tab-layers'}else{'column-icon-layers'})
    $original=(Model).state.layer_tools.editing_layer.id
    Invoke (((Model).state.commands|Where-Object id -eq 'add_layer').label) -Name -Within $configuration
    Wait-Until {(Model).state.layer_tools.editing_layer.id -ne $original} 'Configuration layer action did not create a layer'
    $created=(Model).state.layer_tools.editing_layer.id
    $selector=Control 'configure-control-layers'
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Current ink' -Name -Within $selector).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    $selector.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()
    Wait-Until {(Model).state.layer_tools.editing_layer.id -eq $original} 'Configuration selector did not change the shared editing layer'
    (Control 'configure-layer-opacity-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(0.5)
    Wait-Until {(Model).state.layer_tools.editing_layer.opacity -eq 0.5} 'Configuration opacity did not affect the selected layer'
    if(((Model).state.layers|Where-Object id -eq $created).opacity -ne 1){throw 'Configuration opacity changed the previous target'}
    Toggle 'configure-show-layer_opacity'
    Wait-Until {!(Shown 'layers' 'layer_opacity')} 'Layer opacity visibility did not change'
    Toggle 'configure-show-layer_opacity'
    Wait-Until {Shown 'layers' 'layer_opacity'} 'Layer opacity visibility did not restore'
    Capture 'layers'
    Dismiss 'layers'
    # Navigator is already present in the full editor preset.
    $null=Control 'panel-tab-navigator'
    # Constrain the full editor so the configuration's GPU overview overlaps
    # the opaque Tool Set body; the wide preset leaves empty canvas below it.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1100 -Height 1000
    Start-Sleep -Milliseconds 400
    Capture 'navigator-before' -Composed
    $configuration=Configure 'navigator'
    $null=Control 'navigator-overview' -Within $configuration
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the review'
    $zoom=(Control 'canvas-camera').Current.Name
    Invoke 'navigator-zoom_in' -Within $configuration
    Wait-Until {(Control 'canvas-camera').Current.Name -ne $zoom} 'Configuration Navigator did not update the shared camera'
    Start-Sleep -Milliseconds 250
    Capture 'navigator' -Composed
    $occlusion=Check-OverviewOverlap $configuration
    Dismiss 'navigator'
    Start-Sleep -Milliseconds 250
    Capture 'navigator-after' -Composed
    $restored=[Drawing.Bitmap]::new((Join-Path $run 'navigator-after.png'))
    try{$pixel=$restored.GetPixel($occlusion.x,$occlusion.y);if($pixel.R -gt 245 -and $pixel.G -gt 245 -and $pixel.B -gt 245){throw 'Closing configuration did not restore the lower native panel'}}
    finally{$restored.Dispose()}
    $configuration=Configure 'sizes'
    Invoke 'settings-button'
    $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Light' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq 'light'} 'Theme preference did not update'
    (Control 'CloseButton' -Within $preferences).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)) -and (Control 'drawing-canvas').Current.IsEnabled} 'Preferences did not release the canvas'
    if(!(Model).state.customization.expanded){$configuration=Configure 'sizes'}
    $null=Control 'panel-configuration-sizes'
    Start-Sleep -Milliseconds 400
    Capture 'sizes-light'
    Dismiss 'sizes'
    $configuration=Configure 'toolbar' 'ribbon-grip-toolbar'
    Invoke 'Add Tools…' -Name -Within $configuration
    $null=Control 'tool-picker'
    InvokeDialog 'tool-picker' 'Cancel'
    Wait-Until {$null -eq (Find 'tool-picker')} 'Expansion Add Tools picker did not close'
    if((Model).state.customization.expanded){Dismiss 'toolbar' 'ribbon-grip-toolbar'}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{size_grid=@($docked)+$configured;shared_expansion_width='passed';live_brush_size='passed';control_visibility='passed';retained_configuration='passed';resize='passed';escape_close='passed';layers='passed';editing_layer_selection='passed';targeted_opacity='passed';navigator_configuration='passed';gpu_preview_occlusion_and_restore='passed';theme='passed';toolbar_add_tools='passed';zero_exit='passed';scope='native configuration projection; full visual parity, physical input and presentation are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
