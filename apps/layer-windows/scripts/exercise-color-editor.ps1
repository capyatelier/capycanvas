param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyFind='visible'
$CapyCaptureDelay=250
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/color-editor/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Center([string]$Id){$b=(Control $Id -Arranged).Current.BoundingRectangle;@{x=[int]($b.X+$b.Width/2);y=[int]($b.Y+$b.Height/2)}}
function Shown([string]$Id){$name=(Control $Id).Current.Name;$name.Substring($name.LastIndexOf(' ')+1)}
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
}
function Closed-Editor([string]$Message){Wait-Until {!(Find 'edit-color-apply')} $Message}
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
function Paper-Point{
    $area=(Model).state.camera.work_area;$bounds=(Control 'drawing-canvas').Current.BoundingRectangle
    @{x=[int]($bounds.X+$area[0]+$area[2]/2);y=[int]($bounds.Y+$area[1]+$area[3]/2)}
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
    $revision=(Model).state.document_file.revision

    Invoke-Id 'color-swap'
    Wait-Until {Same (Foreground) @(1,1,1,1)} 'Swapping did not bring the white background forward'
    $white=Foreground
    Open-Editor
    Capture "editor-$Theme" -WithModel
    $forms=@(0..2|ForEach-Object {(Control "edit-color-form-$_").Current.Name})
    if(($forms -join ',') -ne 'RGB,HSB,OKLCH'){throw "Unexpected default formats: $($forms -join ',')"}
    if(((Row 0) -join ',') -ne '255,255,255'){throw "White did not show 255 255 255: $((Row 0) -join ',')"}
    $wheel=(Control 'edit-color-wheel' -Arranged).Current.BoundingRectangle;$values=(Control 'edit-color-0-0' -Arranged).Current.BoundingRectangle
    if($wheel.X -ge $values.X){throw 'A wide window must keep the wheel beside the values'}
    $copies=@('edit-color-hex-copy','edit-color-copy-0','edit-color-copy-2'|ForEach-Object {$b=(Control $_ -Arranged).Current.BoundingRectangle;$b.X+$b.Width})
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

    Invoke 'edit-color-current'
    Wait-Until {(Shown 'edit-color-hex') -eq '#FFFFFF'} 'Current did not revert the draft'

    Invoke 'edit-color-swatches'
    Wait-Until {Sheet-Open} 'The swatch sheet did not open'
    $search=Control 'edit-color-search'
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('zzzz-no-color')
    Wait-Until {Find 'edit-color-sheet-empty'} 'An unmatched search did not explain itself'
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
    Wait-Until {Find 'edit-color-sheet-tile-0'} 'The sheet did not list swatches'
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
    $paper=Paper-Point
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover($paper.x+$i,$paper.y);Start-Sleep -Milliseconds 40}
    $strip=Control 'edit-color-strip' -Arranged
    Capture "strip-$Theme"
    $first=$strip.Current.BoundingRectangle
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover([int]($first.X+20+$i),[int]($first.Y+$first.Height/2));Start-Sleep -Milliseconds 40}
    Wait-Until {$moved=(Control 'edit-color-strip').Current.BoundingRectangle;$moved.X -ne $first.X -or $moved.Y -ne $first.Y} 'The strip did not move away from a hovering pointer'
    Invoke 'edit-color-strip'
    Wait-Until {(Find 'edit-color-apply') -and !(Model).state.color_picker.editor} 'Tapping the strip did not return to Edit Color'
    if((Shown 'edit-color-hex') -ne '#000000'){throw 'Leaving through the strip changed the draft'}
    Invoke 'edit-color-pick'
    Wait-Until {!(Find 'edit-color-apply') -and (Model).state.color_picker.editor} 'The eyedropper did not start picking again'
    for($i=0;$i -lt 6;$i++){[CapyRowPointer]::Hover($paper.x+$i,$paper.y);Start-Sleep -Milliseconds 40}
    Wait-Until {(Model).state.color_picker.sample_point} 'Hovering the canvas did not sample it'
    [CapyRowPointer]::Down('mouse',$paper.x,$paper.y);Start-Sleep -Milliseconds 40;[CapyRowPointer]::Up()
    Wait-Until {Find 'edit-color-apply'} 'Picking did not return to Edit Color'
    Wait-Until {(Shown 'edit-color-hex') -eq '#FFFFFF'} 'The picked paper color did not reach the draft'
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

    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{theme=$Theme;rows='passed';paste_and_copy='passed';native_clipboard_ownership='passed';formats='passed';typing_and_refusal='passed';scrub_and_step='passed';revert='passed';sheet='passed';memory='passed';canvas_pick='passed';use_color='passed';fill_thumbnail='passed';evidence=$run}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    Exit-CapyEnvironment
}
