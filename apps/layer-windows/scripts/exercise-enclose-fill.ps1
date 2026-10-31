param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
$CapyCaptureDelay=250
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/enclose-fill/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$reference=Join-Path $run 'Reference ink.png'
$bitmap=[Drawing.Bitmap]::new(384,256,[Drawing.Imaging.PixelFormat]::Format32bppArgb)
try{
    foreach($left in 40,150,270){for($y=50;$y -lt 170;$y++){for($x=$left;$x -lt $left+90;$x++){
        if($x -lt $left+6 -or $x -ge $left+84 -or $y -lt 56 -or $y -ge 164){$bitmap.SetPixel($x,$y,[Drawing.Color]::Black)}
    }}}
    $bitmap.Save($reference,[Drawing.Imaging.ImageFormat]::Png)
}finally{$bitmap.Dispose()}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Chord([uint16[]]$Modifiers,[uint16]$Code){[CapyRowPointer]::Chord([uint32]$review.Id,$Modifiers,$Code)}
function Command([string]$Id){
    $label=((Model).state.commands|Where-Object id -eq $Id).label
    [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
    Chord @(0x11) 0x4B
    Wait-Until {$search=Find 'command-search';$search -and !$search.Current.IsOffscreen} 'Command search did not open'
    (Find 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($label)
    Wait-Until {(Find 'command-result-0').Current.Name -eq $label} "Command search did not find $Id"
    Key 0x0D
    Wait-Until {!(Find 'command-search')} 'Command search did not close'
}
function Screen([double]$X,[double]$Y){
    $camera=(Model).state.camera;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
    @{x=[int]($bounds.X+($X*$camera.zoom+$camera.translation[0])*$bounds.Width/$camera.viewport[0]);
      y=[int]($bounds.Y+($Y*$camera.zoom+$camera.translation[1])*$bounds.Height/$camera.viewport[1])}
}
function Pixel([double]$X,[double]$Y){
    $at=Screen $X $Y;$sample=[Drawing.Bitmap]::new(1,1);$graphics=[Drawing.Graphics]::FromImage($sample)
    try{$graphics.CopyFromScreen($at.x,$at.y,0,0,[Drawing.Size]::new(1,1));$color=$sample.GetPixel(0,0);@($color.R,$color.G,$color.B)}finally{$graphics.Dispose();$sample.Dispose()}
}
function Red($rgb){$rgb[0] -gt 180 -and $rgb[1] -lt 80 -and $rgb[2] -lt 80}
function Revision{[string](@((Model).state.layers|Where-Object editing)[0].paint_revision)}
function Park{$away=Screen 370 240;[CapyRowPointer]::Hover($away.x,$away.y);Start-Sleep -Milliseconds 300}
function Enclose([switch]$Cancel){
    $points=@(@(20,30),@(315,30),@(315,190),@(20,190),@(20,30))
    $start=Screen $points[0][0] $points[0][1];[CapyRowPointer]::Hover($start.x,$start.y);Start-Sleep -Milliseconds 80
    [CapyRowPointer]::Down('mouse',$start.x,$start.y)
    for($i=1;$i -lt $points.Count;$i++){
        for($step=1;$step -le 8;$step++){
            $x=$points[$i-1][0]+($points[$i][0]-$points[$i-1][0])*$step/8;$y=$points[$i-1][1]+($points[$i][1]-$points[$i-1][1])*$step/8
            $at=Screen $x $y;[CapyRowPointer]::Move($at.x,$at.y);Start-Sleep -Milliseconds 16
        }
    }
    if($Cancel){[CapyRowPointer]::Key(0x1B)}
    [CapyRowPointer]::Up()
    Park
}
function Settled{Wait-Until {$m=Model;$m -and !$m.state.document_file.busy -and $m.brush_ready} 'The drawing did not settle' 30}
function Holes([bool]$Filled){
    Wait-Until {$a=Pixel 85 110;$b=Pixel 195 110;(Red $a) -eq $Filled -and (Red $b) -eq $Filled} $(if($Filled){'The enclosed holes did not fill'}else{'The enclosed holes did not clear'}) 15
}
function Texture-Strength([int]$Percent){
    $entry=Reveal-PanelControl 'tool_settings' 'tool-setting-grain_depth';$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue([string]$Percent);Key 0x0D
    Wait-Until {((Model).state.tool_settings|Where-Object id -eq 'grain_depth').value -eq $Percent/100.} "Texture strength did not accept $Percent%"
}
function Texture-Samples{
    $start=Screen 150 95;$end=Screen 158 103
    $bitmap=[Drawing.Bitmap]::new(($end.x-$start.x),($end.y-$start.y));$graphics=[Drawing.Graphics]::FromImage($bitmap)
    $stream=[IO.MemoryStream]::new();$hash=[Security.Cryptography.SHA256]::Create()
    try{
        $graphics.CopyFromScreen($start.x,$start.y,0,0,$bitmap.Size);$bitmap.Save($stream,[Drawing.Imaging.ImageFormat]::Png)
        [BitConverter]::ToString($hash.ComputeHash($stream.ToArray())).Replace('-','')
    }finally{$hash.Dispose();$stream.Dispose();$graphics.Dispose();$bitmap.Dispose()}
}
function Texture-Path([string]$Tool){
    if($Tool -eq 'fill'){$at=Screen 190 120;[CapyRowPointer]::Down('mouse',$at.x,$at.y);[CapyRowPointer]::Up();return}
    if($Tool -eq 'enclose_fill'){Enclose;return}
    $points=if($Tool -eq 'gradient'){@(@(80,120),@(300,120))}else{@(@(70,70),@(290,70),@(290,180),@(70,180),@(70,70))}
    $at=Screen $points[0][0] $points[0][1];[CapyRowPointer]::Down('mouse',$at.x,$at.y)
    try{foreach($point in $points[1..($points.Count-1)]){$at=Screen $point[0] $point[1];[CapyRowPointer]::Move($at.x,$at.y)}}finally{[CapyRowPointer]::Up()}
}
function Check-Textures{
    Use-PaintWorkspace
    $records=[Collections.Generic.List[object]]::new()
    foreach($paper in @('none','bristol','hot_press')){
        Command 'new_document';Wait-Until {(Model).state.new_drawing} 'New drawing did not open'
        Invoke 'new-drawing-presets-button';Wait-Until {(Model).state.new_drawing.drawer.kind -eq 'presets'} 'Preset browser did not open'
        (New-DrawingControl 'new-drawing-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Full HD')
        Wait-Until {@((Model).state.new_drawing.drawer.tiles).Count -eq 1 -and (Model).state.new_drawing.drawer.tiles[0].name -eq 'Full HD'} 'Digital Full HD preset was not found'
        Invoke 'new-drawing-preset-0';Invoke 'new-drawing-select'
        Wait-Until {!(Model).state.new_drawing.drawer -and (Model).state.new_drawing.values[0] -eq 1920 -and (Model).state.new_drawing.values[1] -eq 1080} 'Full HD did not apply'
        if($null -ne (Model).state.new_drawing.setup.paper.texture){throw 'Digital Full HD retained paper'}
        New-DrawingPixels 384 256
        if($paper -ne 'none'){
            Invoke 'new-drawing-texture';Wait-Until {(Model).state.new_drawing.drawer.kind -eq 'paper'} 'Paper drawer did not open'
            $tiles=@((Model).state.new_drawing.drawer.tiles);$index=@(0..($tiles.Count-1)|Where-Object {$tiles[$_].id -eq $paper})
            if($index.Count -ne 1){throw "Missing paper $paper"}
            Invoke ('new-drawing-paper-tile-'+$index[0])
            Wait-Until {(Model).state.new_drawing.drawer.tiles[$index[0]].selected -and (New-DrawingControl 'new-drawing-paper-relief').Current.IsEnabled} 'Texture paper selection did not enable relief'
            $entry=New-DrawingControl 'new-drawing-paper-relief';$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('0')
            Invoke 'new-drawing-paper-use';Wait-Until {(Model).state.new_drawing.setup.paper.texture -eq $paper -and !(Model).state.new_drawing.drawer} 'Paper did not apply'
        }
        Invoke 'new-drawing-create';Wait-Until {!(Model).state.new_drawing -and (Model).brush_ready -and !(Model).state.document_file.busy} 'Texture drawing did not become ready' 45
        if(($paper -eq 'none') -and @((Model).state.layers|Where-Object content_icon -eq 'layer-paper-symbolic').Count){throw 'Digital Full HD created a Paper layer'}
        Fit-Canvas;Command 'pen'
        $start=Screen 50 50;[CapyRowPointer]::Down('mouse',$start.x,$start.y)
        try{foreach($point in @(@(300,50),@(300,170),@(50,170),@(50,50))){$at=Screen $point[0] $point[1];[CapyRowPointer]::Move($at.x,$at.y)}}finally{[CapyRowPointer]::Up()}
        Settled;Invoke-Id 'layer-reference';Wait-Until {(Model).state.layer_tools.references_selected} 'Texture reference was not marked'
        Invoke-Id 'layer-new';Wait-Until {!@((Model).state.layers|Where-Object editing)[0].reference} 'Texture target did not open'
        Invoke-Id 'color-edit';Wait-Until {Find 'edit-color-apply'} 'Texture color editor did not open'
        Invoke 'edit-color-hex';$entry=Control 'edit-color-hex-entry';$entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#D91A0D');Key 0x0D
        Invoke 'edit-color-apply';Wait-Until {!(Find 'edit-color-apply')} 'Texture color editor did not close'
        Park;$emptyRevision=Revision
        foreach($tool in @('fill','lasso_fill','enclose_fill','gradient')){
            Command $tool;Wait-Until {((Model).state.commands|Where-Object id -eq $tool).selected} "Texture tool did not activate: $tool"
            Settled;Park;$empty=Texture-Samples
            $flat=$null
            foreach($percent in @(0,100,200)){
                Texture-Strength $percent;Texture-Path $tool
                Wait-Until {(Revision) -ne $emptyRevision} "Texture $tool did not paint at $percent%" 30
                Settled;Park;$filled=Texture-Samples
                if($filled -eq $empty){throw "Texture $tool left sampled pixels empty at $percent%"}
                if($percent -eq 0){$flat=$filled}elseif($paper -eq 'none' -and $filled -ne $flat){throw "No paper changed $tool pixels at $percent%"}
                if($paper -ne 'none' -and $percent -eq 200 -and $filled -eq $flat){throw "$paper did not texture $tool at 200%"}
                if($percent -eq 100){Capture "texture-$paper-$tool-$Theme" -WithModel}
                Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $emptyRevision} 'Texture Undo did not restore the target';Settled;Park
                Wait-Until {(Texture-Samples) -eq $empty} 'Texture Undo did not restore pixels'
                Chord @(0x11) 0x59;Wait-Until {(Revision) -ne $emptyRevision} 'Texture Redo did not restore the target';Settled;Park
                Wait-Until {(Texture-Samples) -eq $filled} 'Texture Redo did not restore pixels'
                Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $emptyRevision} 'Texture restoration Undo did not complete';Settled
                $records.Add(@{paper=$paper;tool=$tool;strength=$percent;pixels=$filled;undo_redo='passed'})
                Write-Output "$paper $tool $percent% pixels and Undo/Redo passed"
            }
        }
    }
    $records|ConvertTo-Json -Depth 4|Set-Content (Join-Path $run 'texture-results.json')
    @{theme=$Theme;cases=$records.Count;digital_no_paper='passed';evidence=$run}|ConvertTo-Json
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned enclose fill review $($review.Id): $run"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Enclose fill review did not start' 90
    $native=@{window=$null}
    Wait-Until {$native.window=Owned-DrawingWindow $review;$null -ne $native.window} 'The owned drawing window did not appear'
    $root=$native.window.Root;$drawingWindow=$native.window.Handle
    Maximize-DrawingWindow
    Use-PaintWorkspace
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)

    Open-Project $reference
    Wait-Until {$tab=@((Model).state.tabs|Where-Object active)[0];$tab -and $tab.width -eq 384 -and $tab.height -eq 256 -and (Model).brush_ready} 'The reference image did not open' 45
    Invoke-Id 'layer-reference'
    Wait-Until {(Model).state.layer_tools.references_selected} 'The reference ink did not become a reference'
    Invoke-Id 'layer-new'
    Wait-Until {@((Model).state.layers).Count -ge 2 -and !@((Model).state.layers|Where-Object editing)[0].reference} 'A new paint layer did not open above the reference'
    Command 'fill'
    foreach($scope in 'fill_everywhere','fill_connected'){
        $null=Reveal-PanelControl 'tool_settings' 'tool-action-group-fill-scope'
        (Control 'tool-action-group-fill-scope' -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        (Control ('tool-action-'+$scope) -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Wait-Until {((Model).state.commands|Where-Object id -eq $scope).selected} "Fill did not select $scope"
    }
    Command 'enclose_fill'
    Wait-Until {((Model).state.commands|Where-Object id -eq 'enclose_fill').selected} 'Enclose and Fill did not become the tool'
    $settings=@((Model).state.tool_settings|ForEach-Object id)
    foreach($id in 'tolerance','gap_closing','expansion'){
        if($settings -notcontains $id){throw "Enclose and Fill does not share $id with Fill"}
        $null=Reveal-PanelControl 'tool_settings' ('tool-setting-'+$id)
    }
    foreach($id in 'gap_closing','expansion'){
        $entry=Control ('tool-setting-'+$id);$entry.SetFocus()
        $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('0');Key 0x0D
        Wait-Until {((Model).state.tool_settings|Where-Object id -eq $id).value -eq 0} "Enclose and Fill did not accept $id 0"
    }
    $null=Reveal-PanelControl 'tool_settings' 'tool-action-smooth_region_edges'
    Invoke 'tool-action-smooth_region_edges'
    Wait-Until {!((Model).state.commands|Where-Object id -eq 'smooth_region_edges').selected} 'Smooth edges did not turn off'
    Invoke-Id 'color-edit'
    Wait-Until {Find 'edit-color-apply'} 'Edit Color did not open'
    Invoke 'edit-color-hex';$hex=Control 'edit-color-hex-entry';$hex.SetFocus()
    $hex.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('#D91A0D');Key 0x0D
    Invoke 'edit-color-apply';Wait-Until {!(Find 'edit-color-apply')} 'Edit Color did not close'
    Fit-Canvas;Settled;Park
    Holes $false
    $empty=Revision
    Enclose
    Wait-Until {(Revision) -ne $empty} 'Enclose and Fill did not paint'
    Settled;Park;Holes $true
    foreach($point in @(@(25,110),@(140,110),@(290,110),@(85,195))){if(Red (Pixel $point[0] $point[1])){throw "Enclose and Fill painted outside the enclosed holes at $($point -join ',')"}}
    $ink=Pixel 42 110;if(($ink|Measure-Object -Maximum).Maximum -gt 60){throw "The reference ink did not stay visible: $($ink -join ',')"}
    Capture "enclose-fill-$Theme" -WithModel
    Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $empty} 'One Undo did not remove both holes';Settled;Park;Holes $false
    Chord @(0x11) 0x59;Wait-Until {(Revision) -ne $empty} 'Redo did not restore the fill';Settled;Park;Holes $true
    Chord @(0x11) 0x5A;Wait-Until {(Revision) -eq $empty} 'Undo did not remove the fill again';Settled
    Enclose -Cancel
    Settled;Start-Sleep -Milliseconds 500
    if((Revision) -ne $empty){throw 'Escape did not cancel the enclosure'}
    Park;Holes $false
    Chord @(0x11) 0x59;Wait-Until {(Revision) -ne $empty} 'Cancelling an enclosure lost Redo';Settled;Park;Holes $true
    Use-Workspace 'builtin:workspace:painter' 'Sketch'
    Command 'enclose_fill';Command 'hand'
    $fill=@((Model).header.model.zones|ForEach-Object {$_}|Where-Object {$_.item.control.slot -eq 'fill'})[0]
    if(@((Model).header.items|Where-Object id -eq $fill.id)[0].resolved_control.command -ne 'enclose_fill'){throw 'Sketch Fill lost its remembered tool'}
    $bounds=(Control ('header-item-'+$fill.id) -Arranged).Current.BoundingRectangle
    $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
    foreach($drawer in $false,$true){
        [CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Up()
        if($drawer){Wait-Until {(@((Model).state.customization.drawer.columns|ForEach-Object {$_}) -join ',') -eq 'tools,tool_settings'} 'Sketch Fill did not open Tools and Tool settings'}
        else{Wait-Until {((Model).state.commands|Where-Object id -eq 'enclose_fill').selected} 'Sketch Fill did not restore Enclose and Fill'}
    }
    foreach($index in 0..2){$row=Control ('tool-group-'+$index) -Within (Control 'tool-drawer') -Arranged;if($row.Current.Name -ne @('Fill','Lasso fill','Enclose and Fill')[$index]){throw 'Sketch Fill drawer lost its siblings'}}
    Capture "sketch-fill-$Theme" -WithModel
    Key 0x1B;Wait-Until {$null -eq (Model).state.customization.drawer} 'Sketch Fill drawer did not close'
    $canvas=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::Down('mouse',$x,$y);[CapyRowPointer]::Move([int]($canvas.X+$canvas.Width/2),[int]($canvas.Y+$canvas.Height/2));[CapyRowPointer]::Up()
    $taken=@{bubble=$null}
    Wait-Until {$model=Model;if(!$model){return $false};$bubbles=@($model.state.workspace.layout.bubbles.bubbles);if($bubbles.Count -ne 1){return $false};$taken.bubble=$bubbles[0];$null -ne $taken.bubble} 'Sketch Fill did not take a bubble'
    $bubble=$taken.bubble
    if($bubble.source.control.slot -ne 'fill'){throw "Sketch Fill took a different bubble source: $($bubble.source|ConvertTo-Json -Depth 8 -Compress)"}
    $null=Control ('tool-bubble-'+$bubble.id) -Arranged
    Capture "sketch-fill-bubble-$Theme" -WithModel
    Check-Textures
    if((Model).state.host_error){throw "Enclose and Fill reported $((Model).state.host_error)"}
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;reference='passed';shared_controls='passed';enclosed_holes='passed';exterior_and_crossed='passed';reference_ink='passed';undo_redo='passed';escape='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
