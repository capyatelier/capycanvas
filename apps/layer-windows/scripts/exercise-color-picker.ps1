param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('full','fit-dark','fit-light','sampling-dark','sampling-light')][string]$Case='full')
if($Case -ne 'full'){$Theme=($Case -split '-')[-1]}
$samplingOnly=$Case.StartsWith('sampling-')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'BubbleFixtureNative.cs')
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/color-picker/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Center([string]$Id){$b=(Control $Id -Arranged).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Presentation{try{(Workspace-Root).Current.ItemStatus|ConvertFrom-Json}catch{}}
function Preview{(Presentation).color_preview}
function Tool{(Model).state.layer_tools.tool}
function Picking{(Tool) -like 'pick_*'}
function Foreground{(Model).state.colors.foreground.rgba}
function Light($rgba){$rgba -and $rgba[0] -gt .85 -and $rgba[1] -gt .85 -and $rgba[2] -gt .85}
function Inked($rgba){$rgba -and ($rgba[0]+$rgba[1]+$rgba[2]) -lt 2.4}
function Tile([string]$Kind,[string]$Command){
    foreach($panel in @((Model).panels)){foreach($tile in @($panel.tiles)){
        if($tile.control.kind -eq $Kind -and (!$Command -or $tile.control.command -eq $Command)){return "tile-$($panel.id)-$($tile.id)"}
    }}
}
function Hover-Until($at,[scriptblock]$Condition,[string]$Message,[switch]$Mouse){
    $hover=@{condition=$Condition;i=0}
    Wait-Until {
        if($Mouse){[CapyRowPointer]::Hover($at.x+($hover.i%2),$at.y)}else{[CapyRowPointer]::PenHover($at.x+($hover.i%2),$at.y)}
        $hover.i++;& $hover.condition
    } $Message 8
}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Double-Press($At){
    $elapsed=[CapyRowPointer]::DoubleClick($At.x,$At.y)
    Write-Output "Double press batch submitted in $elapsed ms"
}
function Key([uint16]$Code){(Control 'drawing-canvas').SetFocus();[CapyRowPointer]::KeyAt($Code,$paper.x,$paper.y)}
function Escape-Picker{
    for($i=0;$i -lt 3 -and ((Picking) -or (Model).state.customization.drawer);$i++){Key 0x1B;Start-Sleep -Milliseconds 200}
    Wait-Until {!(Picking) -and !(Model).state.customization.drawer} 'Escape did not end picking and close its drawer'
}
function Choose([string]$Id,[string]$Item){
    $box=Control $Id
    $box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    $choice=@{item=$null}
    Wait-Until {$choice.item=$box.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Item));$null -ne $choice.item} "Missing choice $Item"
    $choice.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Start-Sleep -Milliseconds 150
    try{$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse()}catch{}
}
function Choices([string]$Id){
    $box=Control $Id -Arranged;$b=$box.Current.BoundingRectangle
    Tap @{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}
    Wait-Until {$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded} "Choice did not open after real mouse click: $Id"
    $items=$box.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))
    $names=@($items|ForEach-Object {$_.Current.Name})
    $box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse();Start-Sleep -Milliseconds 150
    $names
}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {
        $found.switch=Find ('workspace-switch-'+$Id);$found.menu=Find 'header-workspace-menu'
        ($found.switch -and !$found.switch.Current.IsOffscreen) -or ($found.menu -and !$found.menu.Current.IsOffscreen)
    } "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch -or $switch.Current.IsOffscreen){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Color-Resize([string]$Name='color-resized'){
    $group=@((Model).layout.groups|Where-Object {$_.active -eq 'color'})[0]
    if(!$group){throw 'Color is not docked in the starting workspace'}
    $edges=@($group.bounds.x,($group.bounds.x+$group.bounds.width))
    $distance={param($d)($edges|ForEach-Object {[Math]::Abs($d.bounds.x+$d.bounds.width*.5-$_)}|Measure-Object -Minimum).Minimum}
    $divider=@((Model).layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.fixed}|Sort-Object {& $distance $_})[0]
    if(!$divider -or (& $distance $divider) -gt 8){throw 'Color has no column divider'}
    $from=Center ('divider-'+$divider.id);$outward=if([Math]::Abs($divider.bounds.x-$edges[0]) -lt [Math]::Abs($divider.bounds.x-$edges[1])){-1}else{1}
    $layout=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
    [CapyRowPointer]::Down('mouse',$from.x,$from.y)
    for($i=1;$i -le 10;$i++){[CapyRowPointer]::Move($from.x+$outward*$i*3,$from.y);Start-Sleep -Milliseconds 16}
    Wait-Until {[Math]::Abs((Center ('divider-'+$divider.id)).x-($from.x+$outward*30)) -le 2} 'Divider did not follow the held contact'
    $held=(Control 'color-wheel').Current.BoundingRectangle
    [CapyRowPointer]::Up()
    Wait-Until {$w=(Control 'color-wheel').Current.BoundingRectangle;[Math]::Abs((Presentation).color_field_pixels-$w.Width) -le 2} 'Released resize did not finish the final wheel raster' 15
    $released=(Control 'color-wheel').Current.BoundingRectangle
    if([Math]::Abs($released.Width-$held.Width) -gt 1 -or [Math]::Abs($released.X-$held.X) -gt 1 -or [Math]::Abs($released.Y-$held.Y) -gt 1){throw 'Releasing the resize moved or resized the color wheel'}
    if(((Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress) -eq $layout){throw 'Dragging the divider did not resize the Color column'}
    Capture $Name
}
function Color-FitHeight{
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
    Invoke 'new_document';New-DrawingPixels 128 96
    (Control 'document-depth').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control '16-bit float HDR' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Invoke 'Create' -Name
    Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and !(Model).state.document_file.busy} 'HDR Color fit drawing did not open' 60
    Invoke 'panel-tab-color';Color-Resize 'color-hdr-resized'
    $controls=Control 'color-controls' -Arranged;$retained=$controls.GetRuntimeId() -join ':'
    $bounds=$controls.Current.BoundingRectangle;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.;$hdr=[bool](Model).color_panel.hdr
    $proof=@{controls=$bounds;scale=$scale;theme=$Theme;measurement=@((Model).panel_measurements|Where-Object panel -eq 'color')[0];request=@{type='fit_layout';width=[Math]::Floor($bounds.Width)/$scale;height=$bounds.Height/$scale;hdr=$hdr};upper_request=@{type='fit_layout';width=[Math]::Floor($bounds.Width)/$scale;height=($bounds.Height+1)/$scale;hdr=$hdr};constrained_request=@{type='fit_layout';width=280;height=184;hdr=$true}}
    try{
        foreach($case in @(@('shared','request'),@('upper','upper_request'),@('constrained','constrained_request'))){$proof[$case[0]]=[CapyBubbleFixtureNative]::ColorQuery((Join-Path $directory 'layer_windows.dll'),($proof[$case[1]]|ConvertTo-Json -Compress))|ConvertFrom-Json}
        $proof.stage=(Control 'color-panel' -Arranged).Current.BoundingRectangle;$proof.wheel=(Control 'color-wheel' -Arranged).Current.BoundingRectangle
        if($proof.constrained.size -ne 159){throw 'The constrained HDR transport did not select the shared 159 DIP stage'}
        Wait-Until {
            $proof.stage=(Control 'color-panel' -Arranged).Current.BoundingRectangle;$proof.wheel=(Control 'color-wheel' -Arranged).Current.BoundingRectangle
            @(@($proof.shared,$proof.upper)|Where-Object {[Math]::Abs($proof.stage.Width-$_.size*$scale) -le 1 -and [Math]::Abs($proof.wheel.Width/$scale-$_.wheel[2]) -le 1}).Count -gt 0
        } 'Native HDR Color stage and wheel did not apply the shared fit at their actual allocation'
        if(($controls.GetRuntimeId() -join ':') -ne $retained){throw 'Color fit rebuilt its native control'}
        Capture 'color-fit-height' -WithModel -Composed
    }finally{$proof|ConvertTo-Json -Depth 18|Set-Content (Join-Path $run 'color-fit-height.json')}
}
function Canvas-Points{
    $area=(Model).state.camera.work_area;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
    $script:center=@{x=[int]($bounds.X+$area[0]+$area[2]/2);y=[int]($bounds.Y+$area[1]+$area[3]/2)}
    $script:paper=@{x=$center.x;y=$center.y-90}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned color picker review $($review.Id): $run"
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Color picker review did not start' 90
    $drawingWindow=$native.window.Handle;$root=$native.window.Root
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace

    if($Case -eq 'full' -or $samplingOnly){
    if(!$samplingOnly){Color-Resize}
    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    Fit-Canvas;Start-Sleep -Milliseconds 300;Canvas-Points
    $order=$null
    foreach($panel in @((Model).panels)){if(@($panel.tiles|Where-Object {$_.control.kind -eq 'brush_size_slider'}).Count){
        $order=@($panel.tiles|ForEach-Object {if($_.control.kind -eq 'command'){$_.control.command}else{$_.control.kind}})
    }}
    if(($order -join ',') -ne 'brush_size_slider,color_picker,brush_strength_slider,undo,redo'){throw "Unexpected Sketch toolbar order: $($order -join ',')"}

    Key 0x49
    Wait-Until {(Tool) -eq 'pick_visible'} 'I did not start temporary picking'
    Key 0x1B
    Wait-Until {(Tool) -eq 'paint'} 'Escape did not restore the previous tool'

    $tile=Tile 'color_picker';$at=Center $tile
    $tooltip=(Control $tile).Current.HelpText
    Double-Press $at
    Wait-Until {$d=(Model).state.customization.drawer;$d -and $d.compact} 'Double press did not open the compact picker drawer'
    if(!(Picking)){throw 'The first press did not start picking'}
    $drawer=(Model).state.customization.drawer
    if((ConvertTo-Json -InputObject $drawer.columns -Compress) -ne '[["tool_settings"]]' -or $drawer.dismissal -ne 'explicit'){throw "Unexpected picker drawer: $($drawer|ConvertTo-Json -Compress -Depth 8)"}
    $sizes=Choices 'picker-setting-size'
    if(($sizes -join '|') -ne 'Single pixel|5 px circle|15 px circle|51 px circle|101 px circle'){throw "Unexpected sample sizes: $($sizes -join '|')"}
    $null=Control 'picker-setting-source'
    Choose 'picker-setting-size' '101 px circle'
    Wait-Until {(Model).state.color_picker.sample_width -eq 101} 'Sample size choice did not reach Rust'
    if(!(Model).state.customization.drawer){throw 'Choosing a sample size closed the explicit drawer'}
    Capture 'sketch-settings'
    Choose 'picker-setting-size' 'Single pixel'
    Wait-Until {(Model).state.color_picker.sample_width -eq 1} 'Single pixel did not reach Rust'
    Choose 'picker-setting-size' '5 px circle'
    Wait-Until {(Model).state.color_picker.sample_width -eq 5} '5 px circle did not reach Rust'
    if($samplingOnly){@{theme=$Theme;names=$sizes;picker=(Model).state.color_picker;drawer=(Model).state.customization.drawer}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'sampling-size.json');Capture 'sampling-size' -WithModel -Composed}
    Escape-Picker

    if(!$samplingOnly){
    $revision=(Model).state.document_file.revision
    [CapyRowPointer]::Down('pen',($center.x-60),$center.y)
    try{for($i=1;$i -le 24;$i++){[CapyRowPointer]::Move(($center.x-60+$i*5),$center.y);Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
    Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'Seed stroke did not finish'
    Wait-PresentedFrame
    $ink=Foreground
    if(Light $ink){throw 'Seed stroke uses a light foreground; picking could not distinguish it from paper'}

    Key 0x49;Wait-Until {Picking} 'I did not start pen picking'
    $colors=(Model).state.colors|ConvertTo-Json -Compress -Depth 20
    Hover-Until $paper {Light (Preview).rgba} 'Pen hover did not preview the paper'
    Hover-Until $center {Inked (Preview).rgba} 'Pen hover did not sample the painted stroke'
    if(((Model).state.colors|ConvertTo-Json -Compress -Depth 20) -ne $colors){throw 'Hover changed the remembered colors'}
    Capture 'glass-hover'
    Hover-Until $paper {Light (Preview).rgba} 'Pen hover did not return to paper'
    [CapyRowPointer]::Down('pen',$paper.x,$paper.y);Start-Sleep -Milliseconds 250
    if(!(Picking) -or (Light (Foreground))){throw 'Pen contact accepted before lift'}
    [CapyRowPointer]::Up($true)
    Wait-Until {(Tool) -eq 'paint' -and (Light (Foreground))} 'Pen lift did not accept the paper color'

    Key 0x49;Wait-Until {Picking} 'I did not start mouse picking'
    Hover-Until $center {Inked (Preview).rgba} 'Mouse hover did not sample the stroke' -Mouse
    [CapyRowPointer]::Down('mouse',$center.x,$center.y)
    Wait-Until {(Tool) -eq 'paint' -and (Inked (Foreground))} 'Mouse press did not accept the stroke color'
    [CapyRowPointer]::Up()
    if((Model).state.document_file.revision -ne ($revision+1) -and !(Model).state.document_file.modified){throw 'Picking changed the drawing'}

    Key 0x49;Wait-Until {Picking} 'I did not start touch picking'
    $colors=(Model).state.colors|ConvertTo-Json -Compress -Depth 20
    Tap $paper 'touch'
    Wait-Until {(Tool) -eq 'paint'} 'Finger tap did not cancel toolbar picking'
    if(((Model).state.colors|ConvertTo-Json -Compress -Depth 20) -ne $colors){throw 'Finger tap changed the colors'}
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    [CapyCanvasTouch]::Initialize([uint32]$review.Id)
    $finger=@{x=$paper.x;y=$paper.y+40}
    [CapyCanvasTouch]::Down(1,$finger.x,$finger.y)
    Wait-Until {(Picking) -and (Light (Preview).rgba)} 'Touch and hold did not start the picker on paper' 6
    $layer=(Model).state.color_picker.layer
    if((Model).state.color_picker.can_sample_layer){
        [CapyCanvasTouch]::Down(2,($finger.x+160),$finger.y)
        Wait-Until {(Model).state.color_picker.layer -ne $layer} 'Second finger did not toggle the picker source'
        Capture 'glass-layer-touch'
        [CapyCanvasTouch]::Up(2)
        [CapyCanvasTouch]::Down(2,($finger.x+160),$finger.y)
        Wait-Until {(Model).state.color_picker.layer -eq $layer} 'Second finger did not restore the picker source'
        [CapyCanvasTouch]::Up(2)
    }
    for($y=$finger.y+3;$y -lt $center.y;$y+=3){
        [CapyCanvasTouch]::Move(1,$finger.x,$y);Start-Sleep -Milliseconds 30
    }
    [CapyCanvasTouch]::Move(1,$finger.x,$center.y)
    Wait-Until {Inked (Preview).rgba} 'Moving the held finger onto the stroke did not sample it'
    $picker=(Model).state.color_picker;$lifted=$picker.sample_point[1]-$picker.loupe_point[1]
    [CapyCanvasTouch]::Up(1)
    Wait-Until {(Tool) -eq 'paint' -and (Inked (Foreground))} 'Lifting the held finger did not accept its sample'
    if($lifted -lt 20){throw "Touch loupe was not lifted above the finger ($lifted px)"}
    [CapyCanvasTouch]::Dispose()
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Paint' 'builtin:workspace:illustrator'
    Fit-Canvas;Start-Sleep -Milliseconds 300;Canvas-Points
    $category=Tile 'command' 'eyedropper';$at=Center $category
    Double-Press $at
    Wait-Until {$d=(Model).state.customization.drawer;$d -and $d.compact} 'Double press did not open the Eyedropper drawer'
    $drawer=(Model).state.customization.drawer
    if((ConvertTo-Json -InputObject $drawer.columns -Compress) -ne '[["brushes"],["tool_settings"]]'){throw "Unexpected Eyedropper drawer: $(ConvertTo-Json -InputObject $drawer.columns -Compress)"}
    $styles=@((Model).state.tool_set.subtools|ForEach-Object {$_.label})
    if(($styles -join '|') -ne 'Color Picker|Eyedropper'){throw "Unexpected picker styles: $($styles -join '|')"}
    Invoke 'tool-subtool-1';Wait-Until {(Model).state.color_picker.style -eq 'eyedropper'} 'Eyedropper style did not reach Rust'
    Capture 'paint-eyedropper-options'
    Invoke 'tool-subtool-0';Wait-Until {(Model).state.color_picker.style -eq 'glass'} 'Color Picker style did not reach Rust'
    Escape-Picker

    $null=Control 'color-wheel'
    Wait-Until {(Model).shaders_ready} 'Startup shaders did not finish' 180
    Key 0x49;Wait-Until {Picking} 'I did not start wheel preview picking'
    Hover-Until $paper {$null -ne (Preview)} 'No preview before sweep'
    $quiet=@{full=-1;since=[Diagnostics.Stopwatch]::StartNew()}
    Hover-Until $paper {$f=(Presentation).full_updates;if($f -ne $quiet.full){$quiet.full=$f;$quiet.since.Restart()};$quiet.since.ElapsedMilliseconds -gt 1200} 'Workspace publications did not settle before the sweep'
    $before=Presentation;$modelBefore=Get-Content -LiteralPath (State-File) -Raw
    for($i=0;$i -lt 45;$i++){[CapyRowPointer]::PenHover(($center.x-90+$i*4),($center.y+[int](30*[Math]::Sin($i/5))));Start-Sleep -Milliseconds 12}
    for($i=0;$i -lt 14;$i++){[CapyRowPointer]::PenHover(($center.x+90+$i%2),$center.y);Start-Sleep -Milliseconds 30}
    $after=Presentation
    if($after.full_updates -ne $before.full_updates){
        $modelBefore|Set-Content (Join-Path $run 'sweep-before.json');Get-Content -LiteralPath (State-File) -Raw|Set-Content (Join-Path $run 'sweep-after.json')
        throw 'Hover rebuilt the retained workspace'
    }
    if($after.motion_updates -le $before.motion_updates){throw 'Hover did not deliver picker previews'}
    Capture 'wheel-preview'
    Escape-Picker
    }
    }
    if(!$samplingOnly){Color-FitHeight}
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Color picker review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Color picker review wrote to stderr'}
    if($Case -eq 'full'){
    $counts={param($p)@{full=$p.full_updates;motion=$p.motion_updates}}
    @{tooltip=$tooltip;touch_lift_pixels=$lifted;touch_reach_pixels=$reached;column_resize='retained wheel scaled during the drag; final raster at release without moving controls';sweep_before=(& $counts $before);sweep_after=(& $counts $after)}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'result.json')
    }
    Write-Output "Color picker acceptance passed: $run"
}catch{
    try{Capture 'failure' -WithModel}catch{}
    try{@{presentation=(Presentation|Select-Object revision,model_revision,full_updates,motion_updates,color_field_pixels,color_preview);picker=(Model).state.color_picker;tool=(Tool)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    Set-Content -LiteralPath (Join-Path $run 'failure.txt') -Value ($_|Out-String)
    throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    Exit-CapyEnvironment
}
