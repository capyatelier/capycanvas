param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -Path (Join-Path $PSScriptRoot 'CanvasTouchDriver.cs')
$null=[CapyCanvasTouch]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/color-picker/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
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
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Owned color picker review exited unexpectedly'};Start-Sleep -Milliseconds 40}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Id,[switch]$Name,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Type $Type;$null -ne $hit.item -and !$hit.item.Current.IsOffscreen} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id){(Control $Id).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Center([string]$Id){$b=(Control $Id).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Presentation{$workspace=Find 'Drawing workspace' -Name;if($workspace){try{$workspace.Current.ItemStatus|ConvertFrom-Json}catch{}}}
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
    $watch=[Diagnostics.Stopwatch]::StartNew();$i=0
    do{
        if($Mouse){[CapyRowPointer]::Hover($at.x+($i%2),$at.y)}else{[CapyRowPointer]::PenHover($at.x+($i%2),$at.y)}
        $i++;if(& $Condition){return};Start-Sleep -Milliseconds 30
    }while($watch.Elapsed.TotalSeconds -lt 8)
    throw $Message
}
function Tap($at,[string]$Device='mouse'){[CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 30;[CapyRowPointer]::Up()}
function Key([uint16]$Code){(Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::KeyAt($Code,$paper.x,$paper.y)}
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
    $box=Control $Id;$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand();Start-Sleep -Milliseconds 200
    $items=$box.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem))
    $names=@($items|ForEach-Object {$_.Current.Name})
    $box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Collapse();Start-Sleep -Milliseconds 150
    $names
}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Switch-Workspace([string]$Name,[string]$Id){
    $found=@{switch=$null;menu=$null}
    Wait-Until {
        $found.switch=Find ('workspace-switch-'+$Name.ToLowerInvariant());$found.menu=Find 'header-workspace-menu'
        ($found.switch -and !$found.switch.Current.IsOffscreen) -or ($found.menu -and !$found.menu.Current.IsOffscreen)
    } "No workspace switcher for $Name" 15
    $switch=$found.switch
    if(!$switch -or $switch.Current.IsOffscreen){Invoke 'header-workspace-menu';$switch=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)}
    $switch.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $Id -and !(Model).windows_workspace.busy} "$Name did not open" 20
}
function Canvas-Points{
    $area=(Model).state.camera.work_area;$bounds=(Control 'Drawing canvas' -Name).Current.BoundingRectangle
    $script:center=@{x=[int]($bounds.X+$area[0]+$area[2]/2);y=[int]($bounds.Y+$area[1]+$area[3]/2)}
    $script:paper=@{x=$center.x;y=$center.y-90}
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned color picker review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Color picker review did not start' 90
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Sketch' 'builtin:workspace:painter'
    if(Find 'canvas-fit'){Invoke 'canvas-fit'};Start-Sleep -Milliseconds 300;Canvas-Points
    $order=$null
    foreach($panel in @((Model).panels)){if(@($panel.tiles|Where-Object {$_.control.kind -eq 'brush_size_slider'}).Count){
        $order=@($panel.tiles|ForEach-Object {if($_.control.kind -eq 'command'){$_.control.command}else{$_.control.kind}})
    }}
    if(($order -join ',') -ne 'brush_size_slider,color_picker,brush_opacity_slider,undo,redo'){throw "Unexpected Sketch toolbar order: $($order -join ',')"}

    Key 0x49
    Wait-Until {(Tool) -eq 'pick_visible'} 'I did not start temporary picking'
    Key 0x1B
    Wait-Until {(Tool) -eq 'paint'} 'Escape did not restore the previous tool'

    $tile=Tile 'color_picker';$at=Center $tile
    $tooltip=(Control $tile).Current.HelpText
    Tap $at;Start-Sleep -Milliseconds 90;Tap $at
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
    Escape-Picker

    $revision=(Model).state.document_file.revision
    [CapyRowPointer]::Down('pen',($center.x-60),$center.y)
    try{for($i=1;$i -le 24;$i++){[CapyRowPointer]::Move(($center.x-60+$i*5),$center.y);Start-Sleep -Milliseconds 8}}finally{[CapyRowPointer]::Up()}
    Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified} 'Seed stroke did not finish'
    Start-Sleep -Milliseconds 300
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
    Wait-Until {(Picking) -and (Light (Preview).rgba)} 'Touch and hold did not start the lifted picker' 6
    $layer=(Model).state.color_picker.layer
    if((Model).state.color_picker.can_sample_layer){
        [CapyCanvasTouch]::Down(2,($finger.x+160),$finger.y)
        Wait-Until {(Model).state.color_picker.layer -ne $layer} 'Second finger did not toggle the picker source'
        Capture 'glass-layer-touch'
        [CapyCanvasTouch]::Up(2)
    }
    $offset=0
    for($i=1;$i -le 40 -and !(Inked (Preview).rgba);$i++){
        [CapyCanvasTouch]::Move(1,$finger.x,($finger.y+$i*3));Start-Sleep -Milliseconds 30;$offset=$i*3
    }
    if(!(Inked (Preview).rgba)){throw 'Moving the held finger did not sample the stroke above it'}
    $lifted=$finger.y+$offset-$center.y
    [CapyCanvasTouch]::Up(1)
    Wait-Until {(Tool) -eq 'paint' -and (Inked (Foreground))} 'Lifting the held finger did not accept its sample'
    if($lifted -lt 20){throw "Touch sample was not lifted above the finger ($lifted px)"}
    [CapyCanvasTouch]::Dispose()
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Switch-Workspace 'Paint' 'builtin:workspace:illustrator'
    if(Find 'canvas-fit'){Invoke 'canvas-fit'};Start-Sleep -Milliseconds 300;Canvas-Points
    $category=Tile 'command' 'eyedropper';$at=Center $category
    Tap $at;Start-Sleep -Milliseconds 90;Tap $at
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
    $before=Presentation;$modelBefore=Get-Content -LiteralPath $script:statePath -Raw
    for($i=0;$i -lt 45;$i++){[CapyRowPointer]::PenHover(($center.x-90+$i*4),($center.y+[int](30*[Math]::Sin($i/5))));Start-Sleep -Milliseconds 12}
    for($i=0;$i -lt 14;$i++){[CapyRowPointer]::PenHover(($center.x+90+$i%2),$center.y);Start-Sleep -Milliseconds 30}
    $after=Presentation
    if($after.full_updates -ne $before.full_updates){
        $modelBefore|Set-Content (Join-Path $run 'sweep-before.json');Get-Content -LiteralPath $script:statePath -Raw|Set-Content (Join-Path $run 'sweep-after.json')
        throw 'Hover rebuilt the retained workspace'
    }
    if($after.color_fields -ne $before.color_fields){throw 'Hover rasterized the color field on the UI thread'}
    if($after.motion_updates -le $before.motion_updates){throw 'Hover did not deliver picker previews'}
    Capture 'wheel-preview'
    Escape-Picker
    [CapyRowPointer]::Verify();[CapyRowPointer]::Dispose()

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if(!$review.WaitForExit(8000)){throw 'Color picker review did not close'}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Color picker review wrote to stderr'}
    $counts={param($p)@{full=$p.full_updates;motion=$p.motion_updates;fields=$p.color_fields}}
    @{tooltip=$tooltip;touch_lift_pixels=$lifted;sweep_before=(& $counts $before);sweep_after=(& $counts $after)}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'result.json')
    Write-Output "Color picker acceptance passed: $run"
}catch{
    try{Capture 'failure'}catch{}
    try{@{presentation=(Presentation|Select-Object revision,model_revision,full_updates,motion_updates,color_fields,color_preview);picker=(Model).state.color_picker;tool=(Tool)}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    Set-Content -LiteralPath (Join-Path $run 'failure.txt') -Value ($_|Out-String)
    throw
}finally{
    [CapyCanvasTouch]::Dispose();[CapyRowPointer]::Dispose()
    if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force -ErrorAction SilentlyContinue}
    foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}}
}
