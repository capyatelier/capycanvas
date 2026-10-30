param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','layout-dark','layout-light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
$layoutOnly=$Theme.StartsWith('layout-');if($layoutOnly){$Theme=$Theme.Substring(7)}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
$CapyCaptureDelay=250
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyRowPointer]::Verify()}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/color-editor/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Center([string]$Id){$b=(Control $Id -Within $editorFrame -Arranged).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Shown([string]$Id){$name=(Control $Id -Within $editorFrame).Current.Name;$name.Substring($name.LastIndexOf(' ')+1)}
function Row([int]$Row){@(0..2|ForEach-Object {Shown "edit-color-$Row-$_"})}
function Foreground{(Model).state.colors.foreground.rgba}
function Artwork-Identity{
 $m=Model;$stamp=@($m.windows_tabs.session_stamps|Where-Object id -eq $m.windows_tabs.selected)[0].stamp
 [ordered]@{drawing=$stamp|Select-Object artwork,checkpoint,revision,working_generation;file=$m.state.document_file|Select-Object revision,modified,location;layers=$m.state.layers|Select-Object id,label,paint_revision,mask_revision,object}|ConvertTo-Json -Depth 20 -Compress
}
function Same($a,$b){$a -and $b -and @(0..3|Where-Object {[Math]::Abs($a[$_]-$b[$_]) -gt .002}).Count -eq 0}
function Open-Editor{
    Invoke-Id 'color-edit'
    Wait-Until {$apply=Find 'edit-color-apply';$apply -and !$apply.Current.IsOffscreen} 'Edit Color did not open'
    $script:editorFrame=Control 'edit-color-dialog-frame'
}
function Closed-Editor([string]$Message){Wait-Until {!(Find 'edit-color-apply')} $Message;$script:editorFrame=$null}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Type-Into([string]$Id,[string]$Text){
    Invoke $Id
    $entry=Control ($Id+'-entry');$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
    Key 0x0D
}
function Choose-Form([int]$Row,[string]$Form){
    Invoke "edit-color-form-$Row"
    $item=Control "edit-color-form-$Row-$Form"
    $pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
    elseif($item.TryGetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern,[ref]$pattern)){$pattern.Select()}
    else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Clipboard-Text{Get-Clipboard -Raw}
function Paste([string]$Text){
    Set-Clipboard -Value $Text
    (Control 'edit-color-current').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),0x56)
}
function Sheet-Open{$sheet=Find 'edit-color-search';$sheet -and !$sheet.Current.IsOffscreen}
function Presentation{try{(Workspace-Root).Current.ItemStatus|ConvertFrom-Json}catch{}}
function Paper-Point{
    $model=Model;$camera=$model.state.camera;$document=@($model.state.tabs|Where-Object active)[0]
    $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle;$area=$camera.work_area
    if(!$document -or $camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Color review requires its unrotated paper'}
    $paper=@{left=$bounds.X+$camera.translation[0];top=$bounds.Y+$camera.translation[1];width=$document.width*$camera.zoom;height=$document.height*$camera.zoom}
    $point=@{x=[int]($paper.left+$paper.width/2);y=[int]($paper.top+$paper.height/2)}
    @{point=$point;paper=$paper;canvas=$bounds;camera=$camera}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'paper-point.json')
    if($point.x -lt [Math]::Max($paper.left,$bounds.X+$area[0])+4 -or $point.x+5 -gt [Math]::Min($paper.left+$paper.width,$bounds.X+$area[0]+$area[2])-4 -or
       $point.y -lt [Math]::Max($paper.top,$bounds.Y+$area[1])+4 -or $point.y -gt [Math]::Min($paper.top+$paper.height,$bounds.Y+$area[1]+$area[3])-4){throw 'Color review sample is outside visible paper'}
    $point
}
$numericInputChecks=[Collections.Generic.List[object]]::new()
$numericFieldIds=@('edit-color-hex')+@(foreach($row in 0..2){foreach($column in 0..2){"edit-color-$row-$column"}})
function Draft-Identity{@($numericFieldIds|ForEach-Object {(Control $_ -Within $editorFrame).Current.Name})|ConvertTo-Json -Compress}
function No-FieldEntry{foreach($id in $numericFieldIds){if(Find ($id+'-entry') -Within $editorFrame){throw "A scrub opened $id for typing"}}}
function Numeric-Input([string]$Device,[ValidateSet('cancel','outside','modifier')][string]$Ending,[ValidateSet('mouse','touch','key','uia')][string]$Activation,[uint16]$Modifier=0){
    $id='edit-color-0-0';$case="$Device-$Ending-$Activation-$Modifier"
    $check=[ordered]@{case=$case;device=$Device;ending=$Ending;activation=$Activation;modifier=$Modifier;status='started'}
    $numericInputChecks.Add($check)
    try{
        Invoke 'edit-color-current';Wait-Until {(Shown 'edit-color-hex') -eq '#FFFFFF'} "$case did not restore its white baseline"
        $baseline=Draft-Identity;$paint=(Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress;$artwork=Artwork-Identity
        $field=Control $id -Within $editorFrame -Arranged;$field.SetFocus();$bounds=$field.Current.BoundingRectangle
        $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
        $x=[int]($bounds.X+$bounds.Width/2);$y=[int]($bounds.Y+$bounds.Height/2)
        $distance=if($Ending -eq 'modifier'){8*$scale}else{[Math]::Max(24*$scale,$bounds.Height/2+8*$scale)}
        $endY=[int]($y+$distance)
        if($Ending -eq 'modifier' -and !$bounds.Contains([System.Windows.Point]::new($x,$endY))){throw "$case modifier release must stay inside the native Button"}
        if($Ending -eq 'outside' -and $bounds.Contains([System.Windows.Point]::new($x,$endY))){throw "$case release did not leave the native Button"}
        $check.before=$baseline;$check.bounds=$bounds.ToString();$check.from=@($x,$y);$check.to=@($x,$endY);$check.runtime_id=$field.GetRuntimeId() -join ':'
        [CapyRowPointer]::Down($Device,$x,$y);[CapyRowPointer]::Move($x,$endY)
        Wait-Until {[int](Shown $id) -lt 255} "$case did not change the shared color draft while scrubbing"
        $moved=Draft-Identity;$check.moved=$moved
        if($Ending -eq 'modifier'){
            [CapyRowPointer]::Hold($Modifier,$true);[CapyRowPointer]::Move($x,$endY);[CapyRowPointer]::Up()
            $check.stationary_updates_after_modifier=1
        }elseif($Ending -eq 'cancel'){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up($Device -eq 'pen')}
        $check.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$check.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds
        if($Ending -eq 'cancel'){
            Wait-Until {(Draft-Identity) -ceq $baseline} "$case did not restore every shared draft readout exactly"
        }else{
            if((Draft-Identity) -cne $moved){throw "$case release changed the acknowledged scrub draft"}
        }
        if($Modifier){[CapyRowPointer]::Hold($Modifier,$false)}
        No-FieldEntry
        $expected=Draft-Identity;$check.after=$expected
        switch($Activation){
            {$_ -in 'mouse','touch'}{$at=$field.Current.BoundingRectangle;[CapyRowPointer]::Down($Activation,[int]($at.X+$at.Width/2),[int]($at.Y+$at.Height/2));[CapyRowPointer]::Up()}
            'key'{Focus-Keyboard $field;$check.activation_focus=$field.Current.HasKeyboardFocus;Key 0x0D}
            'uia'{$field.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
        }
        Wait-Until {$entry=Find ($id+'-entry') -Within $editorFrame;$entry -and $entry.Current.HasKeyboardFocus} "$case swallowed the next $Activation activation"
        Key 0x1B;Wait-Until {!(Find ($id+'-entry') -Within $editorFrame)} "$case could not leave its activated native editor"
        if((Draft-Identity) -cne $expected){throw "$case activation changed the draft"}
        if(((Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress) -cne $paint -or (Artwork-Identity) -cne $artwork){throw "$case changed paint or artwork before Use Color"}
        if($Ending -eq 'cancel'){
            Invoke 'edit-color-apply';Closed-Editor "$case could not accept the canceled draft"
            Wait-Until {((Model).state.colors.foreground|ConvertTo-Json -Depth 12 -Compress) -ceq $paint} "$case did not accept the exact original shared paint definition"
            if((Artwork-Identity) -cne $artwork){throw "$case accepting the restored paint changed artwork"}
            Open-Editor
            $check.shared_paint_rollback='exact'
        }
        $check.status='passed'
    }catch{
        $check.error=$_|Out-String
        try{Capture ("numeric-$case-failure") -WithModel}catch{}
        throw
    }finally{
        if($Modifier){[CapyRowPointer]::Hold($Modifier,$false)}
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
        @($numericInputChecks.ToArray())|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'numeric-input.json')
    }
}
function Check-EditorRegrow([string]$Kind){
    $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
    $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](1000*$scale)) -Height ([int](900*$scale))
    Open-Editor
    $retained=(Control 'edit-color-wheel' -Within $editorFrame).GetRuntimeId() -join ':'
    $receipts=[Collections.Generic.List[object]]::new()
    foreach($stage in @(@{name='tall';width=1000;height=900},@{name='short';width=1000;height=360},@{name='regrown';width=1000;height=900},@{name='wider';width=1180;height=900})){
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int]($stage.width*$scale)) -Height ([int]($stage.height*$scale))
        $arranged=@{box=$null}
        Wait-Until {
            $arranged.box=(Control 'edit-color-wheel' -Within $editorFrame -Arranged).Current.BoundingRectangle
            $side=$arranged.box.Width/$scale
            if($stage.name -eq 'short'){$side -ge 159 -and $side -lt 231}else{[Math]::Abs($side-232) -le 1}
        } 'Edit Color wheel did not shrink and regain its full available size'
        if(((Control 'edit-color-wheel' -Within $editorFrame).GetRuntimeId() -join ':') -ne $retained){throw 'Edit Color resize rebuilt its wheel control'}
        $receipts.Add(@{stage=$stage;wheel=$arranged.box;scale=$scale;kind=$Kind})
        $receipts|ConvertTo-Json -Depth 7|Set-Content (Join-Path $run ("wheel-regrow-$Kind-$Theme.json"))
        Capture ("wheel-$Kind-$($stage.name)-$Theme") -WithModel
    }
    Invoke 'edit-color-cancel';Closed-Editor 'Edit Color layout review did not cancel'
    $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Wait-Until {$window.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized} 'Editor layout review did not restore its window'
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    Write-Output "Owned color editor review $($review.Id): $run"
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Color editor review did not start' 90
    $root=$owned.window.Root;$drawingWindow=$owned.window.Handle
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Start-Sleep -Milliseconds 600
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    Use-PaintWorkspace
    Check-EditorRegrow 'sdr'
    if($layoutOnly){
        $CapyPopups=$true
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'File'
        Invoke 'new_document';New-DrawingPixels 128 96
        (Control 'document-depth').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
        (Control '16-bit float HDR' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Invoke 'Create' -Name
        Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and !(Model).state.document_file.busy} 'HDR layout drawing did not open' 60
        Check-EditorRegrow 'hdr'
        [CapyRowPointer]::Dispose()
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved
        if((Get-Item -LiteralPath $stderr).Length){throw 'Editor layout review wrote stderr'}
        @{theme=$Theme;retained_sdr_hdr_shrink_and_regrow='passed'}|ConvertTo-Json
        return
    }
    $revision=(Model).state.document_file.revision

    Invoke-Id 'color-swap'
    Wait-Until {Same (Foreground) @(1,1,1,1)} 'Swapping did not bring the white background forward'
    $white=Foreground
    Open-Editor
    Capture "editor-$Theme" -WithModel
    $forms=@(0..2|ForEach-Object {(Control "edit-color-form-$_").Current.Name})
    if(($forms -join ',') -ne 'RGB,HSB,OKLCH'){throw "Unexpected default formats: $($forms -join ',')"}
    if(((Row 0) -join ',') -ne '255,255,255'){throw "White did not show 255 255 255: $((Row 0) -join ',')"}
    $wheel=(Control 'edit-color-wheel' -Within $editorFrame -Arranged).Current.BoundingRectangle;$values=(Control 'edit-color-0-0' -Within $editorFrame -Arranged).Current.BoundingRectangle
    if($wheel.X -ge $values.X){throw 'A wide window must keep the wheel beside the values'}
    $copies=@('edit-color-hex-copy','edit-color-copy-0','edit-color-copy-2'|ForEach-Object {$b=(Control $_ -Within $editorFrame -Arranged).Current.BoundingRectangle;$b.X+$b.Width})
    if(($copies|Measure-Object -Maximum).Maximum-($copies|Measure-Object -Minimum).Minimum -gt 2){throw "Copy buttons do not line up: $($copies -join ',')"}

    Paste 'rgb(202 75 53)'
    Wait-Until {(Shown 'edit-color-hex') -eq '#CA4B35'} 'Pasting a CSS color did not set the draft'
    if(((Row 0) -join ',') -ne '202,75,53'){throw 'Pasted color did not update the RGB row'}
    Invoke 'edit-color-copy-0';Wait-Until {(Clipboard-Text) -eq 'rgb(202 75 53)'} 'RGB copy did not use the standard notation'
    Invoke 'edit-color-hex-copy';Wait-Until {(Clipboard-Text) -eq '#CA4B35'} 'Hex copy did not copy the hex'
    if(!(Same (Foreground) $white)){throw 'Editing the draft changed the paint'}
    $artwork=Artwork-Identity
    Set-Clipboard -Value 'native clipboard sentinel'
    (Control 'edit-color-current').SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x43)
    Wait-Until {(Clipboard-Text) -eq '#CA4B35'} 'Ctrl+C outside a color text field did not copy the whole color'
    Set-Clipboard -Value 'native clipboard sentinel'
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x58)
    Start-Sleep -Milliseconds 400
    if((Clipboard-Text) -ne 'native clipboard sentinel' -or (Artwork-Identity) -ne $artwork){throw 'Ctrl+X outside a color text field fell through to the artwork'}
    Invoke 'edit-color-hex'
    $entry=Control 'edit-color-hex-entry';$entry.SetFocus()
    $text=$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41);Key 0x27
    Set-Clipboard -Value 'native clipboard sentinel'
    foreach($key in @(0x43,0x58)){
        [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),$key);Start-Sleep -Milliseconds 200
        if((Clipboard-Text) -ne 'native clipboard sentinel' -or $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ne $text){throw 'Copy or Cut without a native text selection fell through to the color or artwork'}
    }
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x41)
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x58)
    Wait-Until {(Clipboard-Text) -eq $text -and $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq ''} 'Ctrl+X did not cut the selected native color text'
    [CapyRowPointer]::Chord([uint32]$review.Id,@(0x11),0x56)
    Wait-Until {$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $text} 'Ctrl+V did not restore the selected native color text'
    Key 0x1B;Wait-Until {!(Find 'edit-color-hex-entry')} 'Escape did not leave the color text field'
    if((Artwork-Identity) -ne $artwork -or !(Same (Foreground) $white)){throw 'Native color clipboard keys changed the original artwork or paint'}

    Choose-Form 1 'hsl'
    Wait-Until {(Control 'edit-color-form-1').Current.Name -eq 'HSL'} 'The second row did not switch to HSL'
    Type-Into 'edit-color-1-0' '200'
    Wait-Until {(Shown 'edit-color-1-0') -eq '200°'} 'Typing a hue did not commit'
    if((Find 'edit-color-1-0-entry')){throw 'A committed value stayed open for typing'}

    Type-Into 'edit-color-0-0' 'lots'
    Wait-Until {$status=Find 'edit-color-error';$status -and $status.Current.Name} 'A refused value did not explain itself'
    if((Control 'edit-color-apply').Current.IsEnabled){throw 'A refused value left Use Color enabled'}
    Capture "refused-$Theme"
    Key 0x1B
    Wait-Until {!(Find 'edit-color-0-0-entry') -and (Control 'edit-color-apply').Current.IsEnabled} 'Escape did not cancel the refused value'
    if(!(Find 'edit-color-apply')){throw 'Escape in a field closed the dialog'}

    $before=[int](Shown 'edit-color-0-2');$start=Center 'edit-color-0-2'
    [CapyRowPointer]::Down('mouse',$start.x,$start.y)
    foreach($step in 2,6,12,20){[CapyRowPointer]::Move($start.x,$start.y-$step);Start-Sleep -Milliseconds 30}
    [CapyRowPointer]::Up()
    Wait-Until {[int](Shown 'edit-color-0-2') -gt $before} 'Dragging a number up did not raise it'
    if((Find 'edit-color-0-2-entry')){throw 'A drag opened the number for typing'}
    $after=[int](Shown 'edit-color-0-2')
    (Control 'edit-color-0-2').SetFocus();Key 0x28
    Wait-Until {[int](Shown 'edit-color-0-2') -eq $after-1} 'Arrow Down did not step the number'

    foreach($activation in 'mouse','touch','uia'){Numeric-Input 'touch' 'cancel' $activation}
    foreach($device in 'mouse','pen','touch'){foreach($activation in 'key','uia'){Numeric-Input $device 'outside' $activation}}
    foreach($modifier in 0x10,0x11){Numeric-Input 'mouse' 'modifier' 'uia' $modifier}

    Invoke 'edit-color-current'
    Wait-Until {(Shown 'edit-color-hex') -eq '#FFFFFF'} 'Current did not revert the draft'

    Invoke 'edit-color-swatches'
    Wait-Until {Sheet-Open} 'The swatch sheet did not open'
    $search=Control 'edit-color-search'
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('zzzz-no-color')
    Wait-Until {Find 'edit-color-sheet-empty'} 'An unmatched search did not explain itself'
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
    Wait-Until {Find 'edit-color-sheet-tile-0'} 'The sheet did not list swatches'
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $oldPeer=Control 'edit-color-mini-current' -Within $editorFrame -Arranged;$newPeer=Control 'edit-color-mini-new' -Within $editorFrame -Arranged
    $old=$oldPeer.Current.BoundingRectangle;$new=$newPeer.Current.BoundingRectangle;$cancel=(Control 'edit-color-cancel' -Within $editorFrame -Arranged).Current.BoundingRectangle
    $frame=Control 'edit-color-mini-pair' -Within $editorFrame -Arranged
    $frameBounds=$frame.Current.BoundingRectangle
    @{current=$old;new=$new;frame=$frameBounds;frame_class=$frame.Current.ClassName;frame_runtime=($frame.GetRuntimeId() -join ':');cancel=$cancel;scale=$scale}|ConvertTo-Json|Set-Content (Join-Path $run "sheet-footer-$Theme.json")
    if($frameBounds.IsEmpty -or !$frameBounds.Contains($old) -or !$frameBounds.Contains($new) -or ($frameBounds.Width-$old.Width-$new.Width) -le 0 -or ($frameBounds.Width-$old.Width-$new.Width) -gt 4*$scale -or [Math]::Abs(($frameBounds.Height-$cancel.Height)/$scale) -gt .5 -or [Math]::Abs(($frameBounds.Y+$frameBounds.Height/2)-($cancel.Y+$cancel.Height/2)) -gt 1){throw 'The footer Current|New frame is not as tall and centered as the footer buttons'}
    foreach($half in @($old,$new)){
        if([Math]::Abs($half.Width/$scale-38) -gt .5 -or [Math]::Abs(($half.Y+$half.Height/2)-($cancel.Y+$cancel.Height/2)) -gt 1){throw 'The footer Current|New pair is not two 38-DIP halves inside a frame as tall as the footer buttons'}
    }
    if([Math]::Abs($new.Left-$old.Right) -gt 1){throw 'The footer Current|New halves do not touch'}
    Capture "sheet-$Theme"
    Invoke 'edit-color-sheet-tile-0'
    Wait-Until {(Shown 'edit-color-hex') -ne '#FFFFFF'} 'A sheet swatch did not set the draft'
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Ink')
    Invoke 'edit-color-sheet-close'
    Wait-Until {!(Sheet-Open)} 'The swatch sheet did not close'
    Invoke 'edit-color-cancel'
    Closed-Editor 'Cancel did not close Edit Color'
    if(!(Same (Foreground) $white)){throw 'Cancel changed the paint'}
    Wait-Until {$memory=(Model).state.colors.editor;$memory.forms[1] -eq 'hsl' -and $memory.search -eq 'Ink'} 'Closing Edit Color did not save its formats and search'

    Open-Editor
    if((Control 'edit-color-form-1').Current.Name -ne 'HSL'){throw 'Row formats were not remembered'}
    Invoke 'edit-color-swatches'
    Wait-Until {(Sheet-Open) -and (Control 'edit-color-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq 'Ink'} 'Swatch search was not remembered'
    Invoke 'edit-color-sheet-close';Wait-Until {!(Sheet-Open)} 'The swatch sheet did not close'

    Paste '#000000'
    Wait-Until {(Shown 'edit-color-hex') -eq '#000000'} 'Pasting a hex did not set the draft'
    Invoke 'edit-color-pick'
    Wait-Until {!(Find 'edit-color-apply') -and (Model).state.color_picker.editor} 'The eyedropper did not start picking on the canvas'
    $script:editorFrame=$null
    $paper=Paper-Point
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover($paper.x+$i,$paper.y);Start-Sleep -Milliseconds 40}
    $strip=Control 'edit-color-strip' -Arranged
    Capture "strip-$Theme"
    $first=$strip.Current.BoundingRectangle
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover([int]($first.X+20+$i),[int]($first.Y+$first.Height/2));Start-Sleep -Milliseconds 40}
    Wait-Until {$moved=(Control 'edit-color-strip').Current.BoundingRectangle;$moved.X -ne $first.X -or $moved.Y -ne $first.Y} 'The strip did not move away from a hovering pointer'
    Invoke 'edit-color-strip'
    Wait-Until {(Find 'edit-color-apply') -and !(Model).state.color_picker.editor} 'Tapping the strip did not return to Edit Color'
    $script:editorFrame=Control 'edit-color-dialog-frame'
    if((Shown 'edit-color-hex') -ne '#000000'){throw 'Leaving through the strip changed the draft'}
    Invoke 'edit-color-pick'
    Wait-Until {!(Find 'edit-color-apply') -and (Model).state.color_picker.editor} 'The eyedropper did not start picking again'
    $script:editorFrame=$null
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover($paper.x+$i,$paper.y);Start-Sleep -Milliseconds 40}
    $sampleFrame=@{value=$null}
    Wait-Until {$sampleFrame.value=Presentation;$sampleFrame.value.color_preview} 'Hovering the canvas did not publish a sample'
    $sampleFrame=$sampleFrame.value
    $sample=@($sampleFrame.color_preview.rgba|ForEach-Object {[double]$_})
    if($sample.Count -ne 4 -or @($sample|Where-Object {[double]::IsNaN($_) -or [double]::IsInfinity($_)}).Count -or [Math]::Abs($sample[3]-1) -gt .0001 -or [Math]::Max($sample[0],[Math]::Max($sample[1],$sample[2])) -le (1./255)){throw 'Paper sample is not finite, opaque and distinct from the black draft'}
    $sampleHex='#{0:X2}{1:X2}{2:X2}' -f [int][Math]::Round($sample[0]*255),[int][Math]::Round($sample[1]*255),[int][Math]::Round($sample[2]*255)
    @{rgba=$sample;hex=$sampleHex;presentation_revision=$sampleFrame.revision;model_revision=$sampleFrame.model_revision}|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'paper-pick-sample.json')
    [CapyRowPointer]::Down('mouse',$paper.x,$paper.y);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
    Wait-Until {Find 'edit-color-apply'} 'Picking did not return to Edit Color'
    $script:editorFrame=Control 'edit-color-dialog-frame'
    Wait-Until {(Shown 'edit-color-hex') -eq $sampleHex} 'The acknowledged paper sample did not reach the draft'
    if(!(Same (Foreground) $white)){throw 'Picking changed the paint'}
    if((Model).state.document_file.revision -ne $revision){throw 'Picking changed the drawing'}
    Paste '#336699'
    Wait-Until {(Shown 'edit-color-hex') -eq '#336699'} 'Pasting a hex did not set the draft'
    Invoke 'edit-color-apply'
    Closed-Editor 'Use Color did not close Edit Color'
    Wait-Until {Same (Foreground) @((0x33/255),(0x66/255),(0x99/255),1)} 'Use Color did not set the paint'

    $paper=@((Model).state.layers|Where-Object {$_.fill_color})[-1]
    if(!$paper){throw 'The Paper did not publish its fill color'}
    Invoke ("layer-$($paper.id)-content")
    Wait-Until {Find 'edit-color-apply'} 'The Paper thumbnail did not open Edit Color'
    $script:editorFrame=Control 'edit-color-dialog-frame'
    Paste '#DCECFB'
    Wait-Until {(Shown 'edit-color-hex') -eq '#DCECFB'} 'Pasting a hex did not set the fill draft'
    Invoke 'edit-color-apply'
    Closed-Editor 'Use Color did not close the fill editor'
    Wait-Until {$fill=@((Model).state.layers|Where-Object id -eq $paper.id)[0].fill_color;$fill -and $fill.color.rgba[2] -gt .97 -and $fill.color.rgba[0] -lt .9} 'Use Color did not set the Paper fill'
    Capture "paper-$Theme" -WithModel
    $thumbnailModel=Model;$thumbnailPaper=@($thumbnailModel.state.layers|Where-Object id -eq $paper.id)[0]
    $thumbnailRevision=$thumbnailPaper.paint_revision;$thumbnailDocument=Artwork-Identity
    $thumbnailCamera=$thumbnailModel.state.camera|ConvertTo-Json -Depth 8 -Compress
    Wait-Until {
        $current=Model;$samePaper=@($current.state.layers|Where-Object id -eq $paper.id)
        if($samePaper.Count -ne 1 -or $samePaper[0].paint_revision -ne $thumbnailRevision -or (Artwork-Identity) -ne $thumbnailDocument -or ($current.state.camera|ConvertTo-Json -Depth 8 -Compress) -ne $thumbnailCamera){throw 'The Paper item, artwork or camera changed while waiting for its thumbnail'}
        $thumbnail=Find "layer-$($paper.id)-thumbnail"
        $thumbnail -and !$thumbnail.Current.IsOffscreen -and $thumbnail.Current.ItemStatus -eq 'Ready'
    } 'The same Paper thumbnail did not become ready' 45
    $thumbnail=Control "layer-$($paper.id)-thumbnail" -Arranged
    @{layer_id=$paper.id;paint_revision=$thumbnailRevision;artwork=$thumbnailDocument;camera=$thumbnailCamera;status=$thumbnail.Current.ItemStatus;runtime_id=($thumbnail.GetRuntimeId() -join ':');bounds=$thumbnail.Current.BoundingRectangle;visual_review='Required: Ready does not establish thumbnail pixel content'}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'paper-thumbnail-proof.json')
    Capture "paper-thumbnail-ready-$Theme" -WithModel -Composed
    $origin=[CapyWindowCapture+Point]::new()
    if(![CapyWindowCapture]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Paper thumbnail client origin unavailable'}
    $bounds=$thumbnail.Current.BoundingRectangle
    $expected=[Drawing.ColorTranslator]::FromHtml('#DCECFB')
    $shot=[Drawing.Bitmap]::new((Join-Path $run "paper-thumbnail-ready-$Theme.png"))
    try{
        foreach($fx in @(.2,.3,.38)){foreach($fy in @(.2,.3,.38)){
            $x=[int]($bounds.X+$bounds.Width*$fx)-$origin.x;$y=[int]($bounds.Y+$bounds.Height*$fy)-$origin.y
            $pixel=$shot.GetPixel($x,$y)
            if([Math]::Abs([int]$pixel.R-$expected.R) -gt 2 -or [Math]::Abs([int]$pixel.G-$expected.G) -gt 2 -or [Math]::Abs([int]$pixel.B-$expected.B) -gt 2){throw "The Ready Paper thumbnail did not show its fill at ($x,$y): $($pixel.R),$($pixel.G),$($pixel.B)"}
        }}
    }finally{$shot.Dispose()}

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;rows='passed';paste_and_copy='passed';native_clipboard_ownership='passed';formats='passed';typing_and_refusal='passed';scrub_and_step='passed';numeric_input_cases=$numericInputChecks.Count;revert='passed';sheet='passed';memory='passed';canvas_pick='passed';use_color='passed';fill_thumbnail='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
