param([Parameter(Mandatory)][string]$Executable,[ValidateSet('touch','pen','mouse')][string]$Device='touch',[switch]$Overflow,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
if($Device -eq 'mouse'){Add-Type -Path (Join-Path $PSScriptRoot 'PrivateWorkspaceLease.cs')}
if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)}
$CapyEach={if($env:CAPY_PRIVATE_DESKTOP){[CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)};[CapyRowPointer]::Verify()}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/switcher-drag/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$directory=$run;$CapyTraceDirectory=$run
function Storage {(Model).windows_workspace}
function Manager {$w=(Model).windows_workspace;if($w -and ($null -ne $w.page -or $null -ne $w.prompt)){$w}}
function Layout($Value=(Model)) {$Value.state.workspace|ConvertTo-Json -Depth 80 -Compress}
function Choose([string]$Name){
    (Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Settled {
    Wait-Until {$v=Manager;$s=Storage;$null -ne $v -and !$v.loading -and !$v.busy -and !$s.switcher_busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
    if((Storage).switcher_error){throw (Storage).switcher_error}
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Open-Manager {
    Closed
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    $submenu=Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged
    $bounds=$submenu.Current.BoundingRectangle;[CapyRowPointer]::Hover([int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
    $submenu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Manage Workspaces…' -Name -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Manager did not open'
    Settled
}
function Launch([string]$Label){
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/switcher-drag-review.json')
    Write-Output "Owned switcher review $($review.Id) ($Label)"
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$owned.window -and (Model).brush_ready -and (Storage).ready -and !(Storage).switcher_busy} 'Switcher review did not start' 75
    $script:root=$owned.window.Root;$script:drawingWindow=$owned.window.Handle
    [CapyWindowApi]::ShowWindow($drawingWindow,3)|Out-Null
    if(Find 'Test stroke' -Name){throw 'Switcher fixture requires the production UI without smoke controls'}
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -StateDirectory $run -Action Close
    if(!$review.WaitForExit(15000)){throw 'Owned switcher review did not close'}
    if($review.ExitCode -ne 0){throw "Owned switcher review exited $($review.ExitCode)"}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
function Gesture {
    $control=Find 'workspace-manager-items'
    if($control){try{$control.Current.ItemStatus|ConvertFrom-Json}catch{}}
}
function Rows-Arranged {
    $stable=@{signature=$null;count=0}
    Wait-Until {
        $manager=Manager
        $list=Find 'workspace-manager-items'
        if(!$manager -or !$list -or $manager.loading -or $manager.busy){return $false}
        $viewport=$list.Current.BoundingRectangle;$previousTop=[double]::NegativeInfinity;$visible=@()
        foreach($row in $manager.rows){
            $item=Find ('workspace-manager-row-'+$row.id)
            if(!$item){continue}
            $bounds=$item.Current.BoundingRectangle
            if($bounds.IsEmpty -or $bounds.Height -le 0 -or $bounds.Bottom -le $viewport.Top -or $bounds.Top -ge $viewport.Bottom){continue}
            if($bounds.Top -lt $previousTop){$stable.count=0;return $false}
            $previousTop=$bounds.Top
            $visible+=($row.id+':'+$bounds.ToString())
        }
        if(!$visible.Count){return $false}
        $signature=$visible -join '|'
        if($signature -eq $stable.signature){$stable.count++}else{$stable.signature=$signature;$stable.count=0}
        $stable.count -ge 2
    } 'Native row arrangement did not match saved order'
}
function Point([string]$Id,[switch]$Grip,[switch]$Top){
    Rows-Arranged
    $control=Control ($(if($Grip){'workspace-manager-grip-'}else{'workspace-manager-row-'})+$Id)
    $b=$control.Current.BoundingRectangle
    @{x=[int]($b.X+$b.Width*$(if($Grip){.5}else{.4}));y=[int]($b.Y+$b.Height*$(if($Top){.12}else{.5}))}
}
function Press([string]$Id,[switch]$Grip){
    $at=Point $Id -Grip:$Grip
    [CapyRowPointer]::Down($Device,$at.x,$at.y)
}
function Move-To($At){
    [CapyRowPointer]::Move($At.x,$At.y)
}
function Native-TapAt($Control,[double]$Drift,[switch]$Vertical,[double]$Fraction=.5,[int]$MoveSettleMilliseconds=0,$At=$null){
    $bounds=$Control.Current.BoundingRectangle;$x=if($At){[int]$At.x}else{[int]($bounds.X+$bounds.Width*$Fraction)};$y=if($At){[int]$At.y}else{[int]($bounds.Y+$bounds.Height/2)}
    if($At -and !$bounds.Contains($x,$y)){throw 'Exact routed point is outside its arranged native target'}
    $dx=if($Vertical){0}else{[int]($Drift*$scale)};$dy=[int]($Drift*$scale)
    if($x+$dx -ge $bounds.Right -or $y+$dy -ge $bounds.Bottom){throw 'Invalid run: native row/button tap drift leaves its bounds'}
    $trace=@{device=$Device;runtime_id=@($Control.GetRuntimeId());bounds=$bounds.ToString();from=@($x,$y);to=@(($x+$dx),($y+$dy));drift_dip=$Drift;move_settle_ms=$MoveSettleMilliseconds;gesture_before=Gesture}
    [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 35
    if($Drift){[CapyRowPointer]::Move($x+$dx,$y+$dy)}
    if($MoveSettleMilliseconds){Start-Sleep -Milliseconds $MoveSettleMilliseconds}
    if($MoveSettleMilliseconds){$trace.gesture_before_up=Gesture}
    [CapyRowPointer]::Up($Device -eq 'pen')
    $trace.gesture_after=Gesture
    $trace
}
function Preview-Intact {
    $current=Model
    if(!$current){throw 'Owned atomic snapshot is unavailable'}
    if($current.windows_workspace.selected -ne $preview){throw 'Pointer activity changed the selected workspace row'}
    $actual=Layout $current
    if($actual -ne $previewLayout){
        [IO.File]::WriteAllText((Join-Path $run 'expected-preview.json'),$previewLayout)
        [IO.File]::WriteAllText((Join-Path $run 'actual-preview.json'),$actual)
        throw 'Pointer activity changed the workspace preview layout'
    }
}
function Order-Intact($Order,$Revision){
    $current=(Model).windows_workspace
    if(!$current){throw 'Owned atomic snapshot is unavailable'}
    if(($current.order -join '|') -ne ($Order -join '|') -or $current.switcher_revision -ne $Revision){
        @{case=$script:gestureCase;expected_order=$Order;expected_revision=$Revision;actual_order=$current.order;
            actual_revision=$current.switcher_revision;gesture=(Gesture)}|ConvertTo-Json -Depth 8|
            Set-Content (Join-Path $run 'order-failure.json')
        throw 'Incomplete gesture persisted a reorder'
    }
}
function Preview-Select {
    (Control ('workspace-manager-row-'+$preview)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $preview -and !(Manager).loading} 'Preview did not settle'
    $stable=@{layout=$null;count=0}
    Wait-Until {
        $layout=Layout
        if($layout -eq $stable.layout){$stable.count++}else{$stable.layout=$layout;$stable.count=0}
        $stable.count -ge 3
    } 'Native preview measurements did not settle'
    $script:previewLayout=$stable.layout
}
function Source-Id {@((Storage).order|Where-Object {$_ -ne $preview})[-1]}

function Pin-Buttons {
    $id=(Storage).id;$before=@((Storage).switcher.id) -contains $id;$saved=@((Storage).order)
    foreach($expected in @((!$before),$before)){
        $revision=(Storage).switcher_revision
        $trace=Native-TapAt (Control ('workspace-manager-pin-'+$id) -Arranged) 1;$trace.case='pin-with-grip-menu'
        Wait-Until {(Storage).switcher_revision -gt $revision -and !(Storage).switcher_busy} 'Native pin input was not acknowledged'
        if((@((Storage).switcher.id) -contains $id) -ne $expected){throw 'Native pin did not retain its visibility action'}
        if(((Storage).order -join '|') -ne ($saved -join '|') -or (Find 'workspace-manager-move-up')){throw 'Pin input reordered rows or opened the grip menu'}
        Preview-Intact
        $trace.passed=$true;$taps.Add($trace);$taps|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'native-taps.json')
    }
}

function Workspace-TextGeometry([string]$Id,[switch]$Editing){
    $item=Control ('workspace-manager-row-'+$Id) -Arranged
    $spec=@((Manager).rows|Where-Object id -eq $Id)[0]
    $name=if($Editing){Control ('workspace-manager-rename-'+$Id) -Arranged}else{Control $spec.title -Name -Within $item -Type ([System.Windows.Automation.ControlType]::Text) -Arranged}
    $rect=$name.Current.BoundingRectangle;$row=$item.Current.BoundingRectangle
    $glyph=$null;$textPattern=$null
    if($name.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern,[ref]$textPattern)){
        $rectangles=$textPattern.DocumentRange.GetBoundingRectangles()
        if($rectangles.Count -ge 4){$glyph=@($rectangles[0..3])}
    }
    $subtitle=$null
    if($spec.subtitle){$detail=(Control $spec.subtitle -Name -Within $item -Type ([System.Windows.Automation.ControlType]::Text) -Arranged).Current.BoundingRectangle;$subtitle=@($detail.X,$detail.Y,$detail.Width,$detail.Height)}
    @{row=@($row.X,$row.Y,$row.Width,$row.Height);name=@($rect.X,$rect.Y,$rect.Width,$rect.Height);glyph=$glyph;subtitle=$subtitle;editing=[bool]$Editing;title=$spec.title}
}
function Assert-WorkspaceTextGeometry($Before,$After){
    foreach($part in @('row','subtitle')){
        if($null -eq $Before[$part] -and $null -eq $After[$part]){continue}
        if($null -eq $Before[$part] -or $null -eq $After[$part]){throw "Workspace $part disappeared while renaming"}
        foreach($axis in 0..3){if([Math]::Abs($Before[$part][$axis]-$After[$part][$axis]) -gt 1.5){throw "Workspace $part moved while renaming"}}
    }
    if($Before.glyph -and $After.glyph){foreach($axis in 0..1){if([Math]::Abs($Before.glyph[$axis]-$After.glyph[$axis]) -gt 1.5*$scale){throw 'Workspace name glyph origin moved while renaming'}}}
}
function Workspace-GlyphPixels([string]$Path,$Region,[bool]$Bright,[bool]$Selected=$false){
    $origin=[CapyWindowCapture+Point]::new()
    if(![CapyWindowCapture]::ClientToScreen($drawingWindow,[ref]$origin)){throw 'Owned client origin unavailable for glyph measurement'}
    $image=[Drawing.Bitmap]::new($Path)
    try{
        $left=[Math]::Max(0,[int][Math]::Ceiling($Region[0]-$origin.x));$top=[Math]::Max(0,[int][Math]::Floor($Region[1]-$origin.y))
        $right=[Math]::Min($image.Width,[int][Math]::Floor($Region[0]+$Region[2]-$origin.x));$bottom=[Math]::Min($image.Height,[int][Math]::Ceiling($Region[1]+$Region[3]-$origin.y))
        if($Selected){
            $spans=@(for($y=$top;$y -lt $bottom;$y++){
                $xs=@(for($x=$left;$x -lt $right;$x++){$pixel=$image.GetPixel($x,$y);if($pixel.B -gt $pixel.R+35 -and $pixel.B -gt $pixel.G+15){$x}})
                if($xs.Count -ge 3){@{left=$xs[0];right=$xs[-1];y=$y}}
            })
            $edge=$spans|Group-Object {$_.right}|Sort-Object Count -Descending|Select-Object -First 1
            if(!$edge -or $edge.Count -lt 8){throw 'Selected name background unavailable for rendered glyph measurement'}
            $selection=$edge.Group
            $left=($selection.left|Measure-Object -Minimum).Minimum;$right=[int]$edge.Name+1
            $selectedRows=[Collections.Generic.HashSet[int]]::new()
            foreach($span in $selection){$null=$selectedRows.Add($span.y)}

        }
        $minX=$right;$minY=$bottom;$maxX=$left;$maxY=$top;$count=0
        for($y=$top;$y -lt $bottom;$y++){for($x=$left;$x -lt $right;$x++){
            $pixel=$image.GetPixel($x,$y)
            $ink=if($Bright){[Math]::Min($pixel.R,[Math]::Min($pixel.G,$pixel.B)) -gt 190}else{[Math]::Max($pixel.R,[Math]::Max($pixel.G,$pixel.B)) -lt 80}
            if($ink -and (!$Selected -or $selectedRows.Contains($y))){$minX=[Math]::Min($minX,$x);$minY=[Math]::Min($minY,$y);$maxX=[Math]::Max($maxX,$x);$maxY=[Math]::Max($maxY,$y);$count++}
        }}
        if($count -lt 20){throw 'Composed glyph sample did not contain readable name text'}
        @{bounds=@(($minX+$origin.x),($minY+$origin.y),($maxX+$origin.x),($maxY+$origin.y));pixels=$count;bright=$Bright}
    }finally{$image.Dispose()}
}
function Assert-WorkspaceRenderedName($Before,$Editing,[string]$BeforeImage,[string]$EditingImage){
    $left=[Math]::Max($Before.name[0],$Editing.name[0]);$top=[Math]::Min($Before.name[1],$Editing.name[1])
    $width=.65*[Math]::Min($Before.name[2],$Editing.name[2]);$bottom=[Math]::Max($Before.name[1]+$Before.name[3],$Editing.name[1]+$Editing.name[3])
    $region=@($left,$top,$width,($bottom-$top))
    $display=Workspace-GlyphPixels $BeforeImage $region ($Theme -eq 'dark')
    $editor=Workspace-GlyphPixels $EditingImage $region $true $true
    @{region=$region;display=$display;editor=$editor}|ConvertTo-Json -Depth 8|Set-Content ($EditingImage -replace '\.png$','-glyphs.json')
    foreach($axis in @(0,1,3)){if([Math]::Abs($display.bounds[$axis]-$editor.bounds[$axis]) -gt 1.5){throw 'Rendered name glyph origin/baseline moved while renaming'}}
}
function New-WorkspaceButton {
    $control=Control 'workspace-manager-create' -Arranged
    $bounds=$control.Current.BoundingRectangle;$items=(Control 'workspace-manager-items' -Arranged).Current.BoundingRectangle
    $remove=(Control 'workspace-manager-remove' -Arranged).Current.BoundingRectangle
    if($control.Current.Name -notlike 'New Workspace*'){throw 'New workspace icon has no accessible label'}
    if($bounds.Top -lt $items.Bottom-1 -or $bounds.Left -gt $items.Left+24*$scale -or $remove.Left -lt $bounds.Right-1 -or [Math]::Abs($remove.Top-$bounds.Top) -gt 1){throw 'Plus and trash are not below the left side of the list'}
    if([Math]::Abs(($bounds.Top-$items.Bottom)/$scale-6) -gt 1){throw 'Workspace action hover targets do not have6 DIP of space above them'}
    if(Find 'workspace-manager-search' -Visible){throw 'Workspace manager still exposes Search'}
    $cancel=(Control 'Cancel' -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged).Current.BoundingRectangle
    $apply=(Control (Manager).primary -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged).Current.BoundingRectangle
    if($cancel.Right -gt $apply.Left -or $cancel.Width -ge $apply.Width*.9 -or [Math]::Abs($apply.Right-$items.Right) -gt 4*$scale -or [Math]::Abs($cancel.Height-34*$scale) -gt 2 -or [Math]::Abs($apply.Height-34*$scale) -gt 2){throw 'Manager footer is not natural-width Cancel then Switch aligned right at34 DIP'}
    foreach($row in (Manager).rows){
        if(Find ('workspace-manager-options-'+$row.id) -Visible){throw 'Workspace row still has a dots button'}
        $grip=(Control ('workspace-manager-grip-'+$row.id) -Arranged).Current.BoundingRectangle
        $pin=(Control ('workspace-manager-pin-'+$row.id) -Arranged).Current.BoundingRectangle
        if($grip.Left -lt $pin.Right-1){throw 'Reorder grip is not right of the pin'}
    }
    $control
}
function Row-Context([string]$Id){
    $at=Point $Id
    if($Device -eq 'mouse'){[CapyRowPointer]::RightClick($at.x,$at.y)}else{
        if($Device -eq 'pen'){[CapyRowPointer]::PenHover($at.x,$at.y);Start-Sleep -Milliseconds 50}
        [CapyRowPointer]::Down($Device,$at.x,$at.y)
        Wait-Until {(Gesture).phase -eq 'held'} 'Custom row hold was not recognized' 4
        [CapyRowPointer]::Up($Device -eq 'pen')
    }
    Wait-Until {$null -ne (Find 'workspace-manager-rename') -and $null -ne (Find 'workspace-manager-delete')} 'Custom row context did not expose Rename and Delete'
    foreach($id in @('workspace-manager-rename','workspace-manager-delete')){$item=Control $id -Arranged;Wait-Until {$item.Current.IsEnabled} 'Custom context action remained disabled'}
}
function Swipe-Reveal([string]$Id){
    if($Device -notin @('touch','pen')){throw 'Swipe requires touch or pen'}
    $at=Point $Id
    [CapyRowPointer]::Down($Device,$at.x,$at.y);Start-Sleep -Milliseconds 50
    foreach($distance in @(20,40,80,120)){[CapyRowPointer]::Move([int]($at.x-$distance*$scale),$at.y);Start-Sleep -Milliseconds 20}
    [CapyRowPointer]::Up($Device -eq 'pen')
    Wait-Until {(Gesture).phase -eq 'idle' -and !(Gesture).panning} 'Native swipe retained row ownership'
}
function Swipe-DeleteAction([string]$Id){
    $action=Find ('workspace-manager-swipe-delete-'+$Id) -Visible
    $row=Control ('workspace-manager-row-'+$Id) -Arranged
    $spec=@((Manager).rows|Where-Object id -eq $Id)[0].options|Where-Object {$_.action.type -eq 'delete'}|Select-Object -First 1
    if(!$action){$action=Find $spec.label -Name -Within $row -Visible}
    if($action){$null=Native-TapAt $action 0;return}
    $bounds=$row.Current.BoundingRectangle
    $x=[int]($bounds.Right-20*$scale);$y=[int]($bounds.Y+$bounds.Height/2)
    @{case='native-swipe-revealed-strip';row=$Id;label=$spec.label;bounds=$bounds.ToString();point=@($x,$y);gesture=Gesture}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'swipe-action-target.json')
    [CapyRowPointer]::Down($Device,$x,$y);Start-Sleep -Milliseconds 50;[CapyRowPointer]::Up($Device -eq 'pen')
}
function Read-AckNative($Peer,[int[]]$RootRuntime,[switch]$Held){
    if($Held -and ![CapyRowPointer]::Active){throw 'The native acknowledgement contact is no longer Down'}
    [CapyRowPointer]::Verify()
    if([CapyRowPointer]::GetForegroundWindow() -ne $drawingWindow){throw 'Acknowledgement input lost its owned foreground window'}
    $cache=[System.Windows.Automation.CacheRequest]::new()
    $cache.TreeScope=[System.Windows.Automation.TreeScope]::Element
    $cache.TreeFilter=[System.Windows.Automation.Automation]::RawViewCondition
    $cache.AutomationElementMode=[System.Windows.Automation.AutomationElementMode]::Full
    foreach($property in @(
        [System.Windows.Automation.AutomationElement]::RuntimeIdProperty,
        [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
        [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
        [System.Windows.Automation.AutomationElement]::IsEnabledProperty,
        [System.Windows.Automation.AutomationElement]::IsOffscreenProperty,
        [System.Windows.Automation.AutomationElement]::BoundingRectangleProperty)){$cache.Add($property)}
    $nodes=[Collections.Generic.List[object]]::new()
    $node=$Peer.GetUpdatedCache($cache);$found=$false
    for($depth=0;$node -and $depth-lt32;$depth++){
        $info=$node.Cached
        $runtime=[int[]]$node.GetCachedPropertyValue([System.Windows.Automation.AutomationElement]::RuntimeIdProperty)
        if($info.ProcessId-ne$review.Id -or !$info.IsEnabled -or $info.IsOffscreen){throw 'Held target or ancestor is disconnected, disabled, hidden or foreign'}
        $nodes.Add([pscustomobject]@{runtime_id=$runtime;id=$info.AutomationId;process=$info.ProcessId;enabled=$info.IsEnabled;offscreen=$info.IsOffscreen;bounds=$info.BoundingRectangle})
        if(($runtime-join',')-eq($RootRuntime-join',')){$found=$true;break}
        $node=[System.Windows.Automation.TreeWalker]::RawViewWalker.GetParent($node,$cache)
    }
    if(!$found){throw 'Held target no longer has the retained owned root ancestor'}
    [pscustomobject]@{utc=[DateTime]::UtcNow.ToString('o');active=[CapyRowPointer]::Active;nodes=@($nodes)}
}
function Assert-AckNative($Before,$After){
    if($Before.nodes.Count-ne$After.nodes.Count){throw 'Acknowledgement replaced the native ancestor chain'}
    for($index=0;$index-lt$Before.nodes.Count;$index++){
        if(($Before.nodes[$index].runtime_id-join',')-ne($After.nodes[$index].runtime_id-join',')){throw 'Acknowledgement replaced a retained native target or ancestor'}
    }
    if(!$Before.nodes[0].bounds.Equals($After.nodes[0].bounds)){throw 'Acknowledgement moved the held native target'}
}
function Assert-AckClaim($Before,$After,[string]$Id){
    $old=@($Before|Where-Object Id -eq $Id);$now=@($After|Where-Object Id -eq $Id)
    if($old.Count-ne1 -or $now.Count-ne1){throw 'Metadata acknowledgement lost its active workspace claim'}
    if($old[0].Owner-cne$now[0].Owner -or $old[0].Epoch-cne$now[0].Epoch -or $old[0].Fence-cne$now[0].Fence){throw 'Metadata acknowledgement changed active ownership or fence'}
    if(!$now[0].LeaseUntil -or [long]$now[0].LeaseUntil-lt[long]$old[0].LeaseUntil -or [long]$now[0].LeaseUntil-le[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()){throw 'Active workspace lease expired or moved backwards'}
}
function Assert-AckPreview($Model,[string]$Active,[string]$Selected,[string]$ExpectedLayout){
    $workspace=$Model.windows_workspace
    if($workspace.id-ne$Active -or $workspace.selected-ne$Selected -or (Layout $Model)-cne$ExpectedLayout){throw 'Pending Rename changed active workspace, selected preview or exact layout'}
}
function Ack-SavedTitle($Workspace,[string]$Id){
    $row=@($Workspace.rows|Where-Object id -eq $Id)
    if(!$row.Count){$row=@($Workspace.switcher_display|Where-Object id -eq $Id)}
    if($row.Count-eq1){$row[0].title}
}
function Rejected-InlineNameAdmission([string]$Id){
    if($Device -ne 'mouse'){throw 'Inline rejection admission uses the existing mouse manager journey'}
    (Control ('workspace-manager-row-'+$Id) -Arranged).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $Id -and !(Manager).loading} 'Rejected admission custom preview did not settle'
    $original=(Storage).id;$selected=(Manager).selected;$layout=Layout
    $order=@((Storage).order);$pins=@((Storage).switcher.id);$revision=(Storage).switcher_revision
    $originalTitle=@((Manager).rows|Where-Object id -eq $Id)[0].title
    $next=@((Manager).rows|Where-Object {$_.id -ne $selected -and $_.id -ne $Id})[0].id
    if(!$next){throw 'Inline rejection needs an independent follow-up row'}
    $latest=$originalTitle;$observations=[Collections.Generic.List[object]]::new()
    foreach($followUp in @('select','new','delete','apply')){
        Row-Context $Id;Invoke 'workspace-manager-rename'
        Wait-Until {(Manager).prompt_action.type -eq 'rename' -and !(Manager).busy} 'Rejected admission Rename did not mount'
        $entry=Control ('workspace-manager-rename-'+$Id) -Arranged
        $identity=$entry.GetRuntimeId() -join ':'
        if($entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -cne $latest){throw 'Rename did not reopen its exact accepted name'}
        $entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
        [CapyRowPointer]::Key([uint32]$review.Id,13)
        Wait-Until {(Manager).error -and !(Manager).busy -and (Manager).prompt_action.type -eq 'rename'} 'Empty inline Submit did not publish its rejection'
        $trace=[ordered]@{follow_up=$followUp;before_title=$latest;editor_identity=$identity;passed=$false}
        try{
            switch($followUp){
                'select' {(Control ('workspace-manager-row-'+$next) -Arranged).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()}
                'new' {$trace.follow_up_input=Native-TapAt (Control 'workspace-manager-create' -Arranged) 0}
                'delete' {$remove=Control 'workspace-manager-remove' -Arranged;Wait-Until {$remove.Current.IsEnabled} 'Rejected admission footer Delete remained disabled';$trace.follow_up_input=Native-TapAt $remove 0}
                'apply' {$trace.follow_up_input=Native-TapAt (Control (Manager).primary -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged) 0}
            }
            if($followUp -eq 'select'){
                Wait-Until {
                    $accepted=(Manager).selected
                    $accepted -eq $selected -and (Control ('workspace-manager-row-'+$accepted)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected -and
                    !(Control ('workspace-manager-row-'+$next)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected
                } 'Follow-up Select did not restore native selection to the accepted shared preview'
            }
            $pin=Control ('workspace-manager-pin-'+$selected) -Arranged
            $pinBefore=@((Storage).switcher.id) -contains $selected
            $beforePinRevision=(Storage).switcher_revision;$changedPins=@($order|Where-Object {if($_ -eq $selected){!$pinBefore}else{$pins -contains $_}})
            $null=Native-TapAt $pin 0
            Wait-Until {
                $w=Storage
                !$w.switcher_busy -and $w.switcher_revision -eq ($beforePinRevision+1) -and (($w.switcher.id -join '|') -ceq ($changedPins -join '|'))
            } 'A native pin edit after the follow-up did not publish its exact change'
            $changedPinRevision=(Storage).switcher_revision
            if((Manager).prompt_action.type -ne 'rename' -or (Manager).prompt_action.value -ne $Id){throw 'The acknowledged pin edit followed navigation that replaced Rename'}
            $null=Native-TapAt $pin 0
            Wait-Until {
                $w=Storage
                !$w.switcher_busy -and $w.switcher_revision -eq ($changedPinRevision+1) -and (($w.switcher.id -join '|') -ceq ($pins -join '|'))
            } 'Restoring the native pin did not publish its exact change'
            $revision=(Storage).switcher_revision
            $trace.pin_changed_revision=$changedPinRevision;$trace.pin_restored_revision=$revision
                Wait-Until {$m=Manager;$m -and !$m.busy -and $m.prompt_action.type -eq 'rename' -and $m.prompt_action.value -eq $Id} 'Follow-up replaced the rejected Rename prompt'
            $current=Control ('workspace-manager-rename-'+$Id) -Arranged
            if(($current.GetRuntimeId() -join ':') -cne $identity){throw 'Rejected Rename replaced its native editor'}
            if($current.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -cne ''){throw 'Follow-up changed the rejected empty draft'}
            if((Manager).selected -ne $selected -or (Storage).id -ne $original -or (Layout) -cne $layout){throw 'Rejected name changed the accepted preview or active workspace'}
            if(!((Control ('workspace-manager-row-'+$selected)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected)){throw 'Native selected row differs from the retained accepted preview'}
            if($next -ne $selected -and (Control ('workspace-manager-row-'+$next)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected){throw 'Rejected row selection remained optimistically selected'}
            if(((Storage).order -join '|') -cne ($order -join '|') -or ((Storage).switcher.id -join '|') -cne ($pins -join '|') -or (Storage).switcher_revision -ne $revision){throw 'Rejected navigation changed order or pins'}
            $trace.model=Manager;$trace.native_selected=$selected
            $current.SetFocus();$current.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Inline')
            [CapyRowPointer]::Key([uint32]$review.Id,0x23)
            [CapyRowPointer]::Key([uint32]$review.Id,0x20)
            Wait-Until {$current.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -ceq 'Inline ' -and (Manager).prompt_action.type -eq 'rename'} 'Space did not edit the exact name draft'
            $latest='Inline admission after '+$followUp
            $current.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($latest)
            [CapyRowPointer]::Key([uint32]$review.Id,13)
            Wait-Until {$null -eq (Manager).prompt -and @((Manager).rows|Where-Object id -eq $Id)[0].title -ceq $latest} 'Valid retry did not save its exact multiword name'
            if((Manager).error -or (Manager).selected -ne $selected -or (Layout) -cne $layout){throw 'Valid retry lost its selected preview or error recovery'}
            $trace.accepted_title=$latest;$trace.passed=$true
        }finally{$trace.final_model=Manager;try{$trace.final_native_selected=(Control ('workspace-manager-row-'+$selected)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected;$trace.final_native_next=(Control ('workspace-manager-row-'+$next)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected}catch{$trace.native_query_error=$_.Exception.Message};$observations.Add($trace);$observations|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run 'inline-rejected-admission.json')}
    }
    Choose (Manager).primary;Closed
    Wait-Until {(Storage).id -eq $selected -and !(Storage).busy} 'Valid inline admission Apply did not adopt the selected preview'
    Close;[CapyRowPointer]::Dispose();Launch 'inline-admission-restart'
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if((Storage).id -ne $selected -or ((Storage).order -join '|') -cne ($order -join '|') -or ((Storage).switcher.id -join '|') -cne ($pins -join '|')){throw 'Restart lost accepted workspace, order or pins'}
    Open-Manager
    if(@((Manager).rows|Where-Object id -eq $Id)[0].title -cne $latest){throw 'Restart lost the exact accepted multiword name'}
    Row-Context $Id;Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'Restoring fixture title did not mount Rename'
    $entry=Control ('workspace-manager-rename-'+$Id) -Arranged
    $entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($originalTitle)
    [CapyRowPointer]::Key([uint32]$review.Id,13)
    Wait-Until {$null -eq (Manager).prompt -and @((Manager).rows|Where-Object id -eq $Id)[0].title -ceq $originalTitle} 'Fixture title did not restore'
    (Control ('workspace-manager-row-'+$original)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $original -and !(Manager).loading} 'Original active preview did not restore after restart'
    Choose (Manager).primary;Closed
    Wait-Until {(Storage).id -eq $original -and !(Storage).busy} 'Original active workspace did not restore after restart'
    Open-Manager;Preview-Select
    if(((Storage).order -join '|') -cne ($order -join '|') -or ((Storage).switcher.id -join '|') -cne ($pins -join '|')){throw 'Fixture cleanup changed order or pins'}
}

function Read-AckFailureNode($Peer,$Cache){
    if(!$Peer){return $null}
    $node=$Peer.GetUpdatedCache($Cache);$info=$node.Cached
    $value=$node.GetCachedPropertyValue([System.Windows.Automation.ValuePattern]::ValueProperty,$true)
    $selected=$node.GetCachedPropertyValue([System.Windows.Automation.SelectionItemPattern]::IsSelectedProperty,$true)
    [ordered]@{runtime_id=@($node.GetCachedPropertyValue([System.Windows.Automation.AutomationElement]::RuntimeIdProperty));id=$info.AutomationId;process=$info.ProcessId;focused=$info.HasKeyboardFocus;enabled=$info.IsEnabled;offscreen=$info.IsOffscreen;bounds=$info.BoundingRectangle.ToString();value=$(if($value-is[string]){$value});selected=$(if($selected-is[bool]){$selected});item_status=$info.ItemStatus}
}
function Held-Acknowledgement([string]$Id,[ValidateSet('pin','apply','row')][string]$Target,[bool]$AckBeforeRelease){
    if($Device-ne'mouse'){throw 'Forced acknowledgement ordering requires the existing mouse journey'}
    $original=(Storage).id;$selected=$preview;$layout=$previewLayout
    $next=@((Manager).rows|Where-Object {$_.id-ne$original -and $_.id-ne$selected -and $_.id-ne$Id})[0].id
    if(!$next){throw 'No independent row exists for the acknowledgement contact'}
    Row-Context $Id;Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type-eq'rename'} 'Acknowledgement Rename did not open inline'
    $title=@((Manager).rows|Where-Object id -eq $Id)[0].title
    $entry=Control ('workspace-manager-rename-'+$Id) -Arranged
    $peer=if($Target-eq'pin'){Control ('workspace-manager-pin-'+$selected) -Arranged}elseif($Target-eq'row'){Control ('workspace-manager-row-'+$next) -Arranged}else{Control (Manager).primary -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged}
    $rootRuntime=$root.GetRuntimeId()
    $before=Read-AckNative $peer $rootRuntime
    $bounds=$before.nodes[0].bounds
    if($bounds.IsEmpty -or $bounds.Width-le0 -or $bounds.Height-le0){throw 'Acknowledgement target is not arranged'}
    $order=@((Storage).order);$pinBefore=@((Storage).switcher.id)-contains$selected
    $claims=[PrivateWorkspaceLease]::ReadClaims($run,$env:CAPY_STORAGE_DIR)
    Assert-AckClaim $claims $claims $original
    $owner=@($claims|Where-Object Id -eq $original)[0]
    $failureCache=$null;$failureAccepted=$null;$failureItems=$null;$failureStatePath=$null
    if($Target-eq'row'){
        $failureCache=[System.Windows.Automation.CacheRequest]::new()
        $failureCache.TreeScope=[System.Windows.Automation.TreeScope]::Element
        $failureCache.TreeFilter=[System.Windows.Automation.Automation]::RawViewCondition
        foreach($property in @(
            [System.Windows.Automation.AutomationElement]::RuntimeIdProperty,
            [System.Windows.Automation.AutomationElement]::AutomationIdProperty,
            [System.Windows.Automation.AutomationElement]::ProcessIdProperty,
            [System.Windows.Automation.AutomationElement]::HasKeyboardFocusProperty,
            [System.Windows.Automation.AutomationElement]::IsEnabledProperty,
            [System.Windows.Automation.AutomationElement]::IsOffscreenProperty,
            [System.Windows.Automation.AutomationElement]::BoundingRectangleProperty,
            [System.Windows.Automation.AutomationElement]::ItemStatusProperty,
            [System.Windows.Automation.ValuePattern]::ValueProperty,
            [System.Windows.Automation.SelectionItemPattern]::IsSelectedProperty)){$failureCache.Add($property)}
        $failureAccepted=Control ('workspace-manager-row-'+$selected) -Arranged
        $failureItems=Control 'workspace-manager-items' -Arranged
        $failureStatePath=State-File
    }
    $name='Accepted '+$Target+' '+$AckBeforeRelease+' '+[Guid]::NewGuid().ToString('N').Substring(0,8)
    $entry.SetFocus();$entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($name)
    $trace=[ordered]@{target=$Target;ack_before_release=$AckBeforeRelease;before=$before;claims_before=$claims;name=$name;order_before=$order;passed=$false}
    $lease=$null;$cleanupError=$null
    try{
        $lease=[PrivateWorkspaceLease]::new($run,$env:CAPY_STORAGE_DIR)
        [CapyRowPointer]::Down('mouse',[int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
        $pending=@{model=$null}
        Wait-Until {
            if($lease.Expired){throw 'Acknowledgement gate expired before pending Rename was observed'}
            if(![CapyRowPointer]::Active){throw 'Pending Rename lost its native Down contact'}
            $pending.model=Model
            $w=$pending.model.windows_workspace
            $w.busy -and $w.prompt_action.type-eq'rename' -and @($w.rows|Where-Object id -eq $Id)[0].title-eq$title
        } 'Real Rename did not become pending behind the owned database gate' 3
        Assert-AckPreview $pending.model $original $selected $layout
        $trace.pending_native=Read-AckNative $peer $rootRuntime -Held
        Assert-AckNative $before $trace.pending_native
        $trace.pending_model=$pending.model.windows_workspace
        $trace.claims_pending=[PrivateWorkspaceLease]::ReadClaims($run,$env:CAPY_STORAGE_DIR)
        Assert-AckClaim $claims $trace.claims_pending $original
        if($lease.Expired -or $lease.ElapsedMilliseconds-ge3500){throw 'Pending native observations exhausted the bounded acknowledgement gate'}
        $trace.gate_release_ms=$lease.ElapsedMilliseconds
        if(!$AckBeforeRelease){
            [CapyRowPointer]::Up()
            if([CapyRowPointer]::Active){throw 'Release-before-acknowledgement retained native Down'}
            $model=Model;Assert-AckPreview $model $original $selected $layout
            $trace.released_before_ack=$model.windows_workspace
        }
        $lease.Dispose();$lease=$null
        $ack=@{model=$null}
        Wait-Until {
            $ack.model=Model;$w=$ack.model.windows_workspace
            !$w.busy -and (Ack-SavedTitle $w $Id)-eq$name
        } 'Rename acknowledgement did not publish its exact saved name'
        $trace.ack_model=$ack.model.windows_workspace
        if($AckBeforeRelease){
            $trace.ack_native=Read-AckNative $peer $rootRuntime -Held
            Assert-AckNative $before $trace.ack_native
            if($Target-ne'row'){
                Assert-AckPreview $ack.model $original $selected $layout
                if((@($ack.model.windows_workspace.switcher.id)-contains$selected)-ne$pinBefore){throw 'A native button acted before its held release'}
            }elseif($ack.model.windows_workspace.id-ne$original -or $ack.model.windows_workspace.selected-notin@($selected,$next)){throw 'Acknowledged native row selected a foreign workspace'}
            $trace.claims_at_held_ack=[PrivateWorkspaceLease]::ReadClaims($run,$env:CAPY_STORAGE_DIR)
            Assert-AckClaim $claims $trace.claims_at_held_ack $original
            [CapyRowPointer]::Up()
        }
        if($Target-eq'apply'){
            Closed
            Wait-Until {(Storage).id-eq$selected -and !(Storage).busy} 'Held Apply did not adopt the selected preview'
        }else{
            Settled
            if((Storage).id-ne$original){throw 'Held metadata or row action switched the active workspace'}
            if($Target-eq'pin'){
                if((@((Storage).switcher.id)-contains$selected)-eq$pinBefore){throw 'Held pin lost its visibility toggle'}
                Preview-Intact
            }else{Wait-Until {(Manager).selected-eq$next -and !(Manager).loading} 'Held row did not select its exact target'}
        }
        if(((Storage).order-join'|')-cne($order-join'|')){throw 'Held acknowledgement reordered workspaces'}
        $finalClaims=[PrivateWorkspaceLease]::ReadClaims($run,$env:CAPY_STORAGE_DIR)
        $trace.claims_after=$finalClaims
        if(@($finalClaims|Where-Object {$_.Owner-cne$owner.Owner -or $_.Epoch-cne$owner.Epoch}).Count){throw 'Acknowledgement introduced a foreign workspace owner'}
        if(@($finalClaims|Where-Object Id -eq $Id).Count){throw 'Completed metadata Rename retained a temporary non-active claim'}
        if($Target-ne'apply'){Assert-AckClaim $claims $finalClaims $original}
        else{
            $new=@($finalClaims|Where-Object Id -eq $selected)
            if($new.Count-ne1 -or [long]$new[0].LeaseUntil-le[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()){throw 'Apply did not retain its adopted workspace lease'}
            if(@($finalClaims|Where-Object Id -eq $original).Count){throw 'Apply retained the outgoing workspace claim'}
        }
        if($Target-eq'pin'){
            $revision=(Storage).switcher_revision
            $null=Native-TapAt $peer 0
            Wait-Until {(Storage).switcher_revision-gt$revision -and !(Storage).switcher_busy -and ((@((Storage).switcher.id)-contains$selected)-eq$pinBefore)} 'Ordinary next pin click was swallowed'
            Preview-Intact
        }elseif($Target-eq'row'){
            $null=Native-TapAt (Control ('workspace-manager-row-'+$selected) -Arranged) 0
            Wait-Until {(Manager).selected-eq$selected -and !(Manager).loading} 'Ordinary next row click was swallowed'
            Preview-Select
        }else{
            Open-Manager
            (Control ('workspace-manager-row-'+$original)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
            Wait-Until {(Manager).selected-eq$original -and !(Manager).loading} 'Acknowledgement setup did not restore original preview'
            Settled;Choose (Manager).primary;Closed
            Wait-Until {(Storage).id-eq$original -and !(Storage).busy} 'Acknowledgement setup did not restore original active workspace'
            Open-Manager
            $cancel=Control 'Cancel' -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged
            $null=Native-TapAt $cancel 0;Closed
            Open-Manager;Preview-Select
        }
        $trace.passed=$true
    }catch{
        $trace.error=($_|Out-String)+$_.ScriptStackTrace
        if($Target-eq'row' -and $_.Exception.Message-ceq'Real Rename did not become pending behind the owned database gate'){
            $cost=[Diagnostics.Stopwatch]::StartNew()
            $observed=[ordered]@{utc=[DateTime]::UtcNow.ToString('o');phase='before finally DriverCancel and lease Dispose';dispatch_scope='Published shared controller status only; no normal-app outbound callback trace exists';driver_active_before=[CapyRowPointer]::Active;foreground=[CapyRowPointer]::GetForegroundWindow().ToInt64();expected_window=$drawingWindow.ToInt64();gate_elapsed_before=$lease.ElapsedMilliseconds;gate_expired_before=$lease.Expired;last_polled_workspace=$pending.model.windows_workspace;native=@()}
            $trace.failure_before_cleanup=$observed
            try{
                $observed.raw_snapshot_path=$failureStatePath
                try{$observed.raw_snapshot=[IO.File]::ReadAllText($failureStatePath)}catch{$observed.raw_snapshot_error=$_.Exception.Message}
                $focused=$null;try{$focused=[System.Windows.Automation.AutomationElement]::FocusedElement}catch{$observed.focus_error=$_.Exception.Message}
                foreach($item in @(@{kind='focus';peer=$focused},@{kind='entry';peer=$entry},@{kind='target';peer=$peer},@{kind='accepted';peer=$failureAccepted},@{kind='list';peer=$failureItems})){
                    if($cost.ElapsedMilliseconds-ge500 -or $lease.Expired -or ![CapyRowPointer]::Active){$observed.native_stop='500ms cooperative budget, expired gate or released contact';break}
                    $part=[ordered]@{kind=$item.kind;started_ms=$cost.ElapsedMilliseconds;gate_elapsed_before=$lease.ElapsedMilliseconds;driver_active_before=[CapyRowPointer]::Active;gate_expired_before=$lease.Expired}
                    try{$part.node=Read-AckFailureNode $item.peer $failureCache}catch{$part.error=$_.Exception.Message}
                    $part.finished_ms=$cost.ElapsedMilliseconds;$part.gate_elapsed_after=$lease.ElapsedMilliseconds;$part.driver_active_after=[CapyRowPointer]::Active;$part.gate_expired_after=$lease.Expired
                    $observed.native+=@($part)
                }
            }catch{$observed.capture_error=$_.Exception.Message}finally{
                $observed.elapsed_ms=$cost.ElapsedMilliseconds;$observed.driver_active_after=[CapyRowPointer]::Active;$observed.gate_elapsed_after=$lease.ElapsedMilliseconds;$observed.gate_expired_after=$lease.Expired
            }
        }
        throw
    }finally{
        if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}
        if($lease){try{$lease.Dispose()}catch{$cleanupError=$_|Out-String;$trace.gate_cleanup_error=$cleanupError}}
        [IO.File]::WriteAllText((Join-Path $run ('held-ack-'+$Target+'-'+$AckBeforeRelease+'.json')),($trace|ConvertTo-Json -Depth 35),[Text.UTF8Encoding]::new($false))
        if($cleanupError -and !$trace.error){throw $cleanupError}
    }
}

function Held-Button($Control,[string]$Case){
    $bounds=$Control.Current.BoundingRectangle
    if(!$Control.Current.IsEnabled){throw "$Case is disabled before contact"}
    $trace=@{case=$Case;device=$Device;bounds=$bounds.ToString();enabled_before=$Control.Current.IsEnabled;passed=$false}
    try{
        [CapyRowPointer]::Down($Device,[int]($bounds.X+$bounds.Width/2),[int]($bounds.Y+$bounds.Height/2))
        Start-Sleep -Milliseconds 200
        $trace.enabled_before_up=$Control.Current.IsEnabled;$trace.workspace_before_up=Storage
        [CapyRowPointer]::Up($Device -eq 'pen')
        if(!$trace.enabled_before_up){throw "$Case disabled while native contact was held"}
        $trace.passed=$true
    }finally{
        [CapyRowPointer]::Cancel()
        $taps.Add($trace);$taps|ConvertTo-Json -Depth 25|Set-Content (Join-Path $run 'native-taps.json')
    }
}
function Held-Rename([string]$Id){
    Row-Context $Id;Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'Held pin rename did not open'
    $entry=Control ('workspace-manager-rename-'+$Id) -Arranged;$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Held pin workspace')
    $pin=Control ('workspace-manager-pin-'+$active) -Arranged
    $before=@((Manager).rows|Where-Object id -eq $active)[0].pin.selected;$revision=(Storage).switcher_revision
    Held-Button $pin 'rename-held-pin'
    Wait-Until {$null -eq (Manager).prompt -and (Storage).switcher_revision -gt $revision -and @((Manager).rows|Where-Object id -eq $Id)[0].title -eq 'Held pin workspace'} 'Held pin lost its click or inline name'
    if(@((Manager).rows|Where-Object id -eq $active)[0].pin.selected -eq $before){throw 'Held pin did not toggle'}
    Preview-Intact
    $revision=(Storage).switcher_revision
    $pin.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Storage).switcher_revision -gt $revision -and !(Storage).switcher_busy} 'Held pin restoration did not settle'
    Row-Context $Id;Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'Held Apply rename did not open'
    $entry=Control ('workspace-manager-rename-'+$Id) -Arranged;$entry.SetFocus()
    $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Held Apply workspace')
    $apply=Control (Manager).primary -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button) -Arranged
    Held-Button $apply 'rename-held-apply'
    Closed
    Wait-Until {(Storage).id -eq $preview -and !(Storage).busy} 'Held Apply did not commit the selected preview'
    Open-Manager
    if(@((Manager).rows|Where-Object id -eq $Id)[0].title -ne 'Held Apply workspace'){throw 'Held Apply lost inline name'}
    (Control ('workspace-manager-row-'+$active)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $active -and !(Manager).loading} 'Original active row did not preview after held Apply'
    Settled;Choose (Manager).primary;Closed
    Wait-Until {(Storage).id -eq $active -and !(Storage).busy} 'Held Apply fixture did not restore original active workspace'
    Open-Manager;Preview-Select
    if($Device-eq'mouse'){
        foreach($target in @('pin','apply','row')){
            foreach($ackBeforeRelease in @($false,$true)){Held-Acknowledgement $Id $target $ackBeforeRelease}
        }
    }
    Write-Output "$Device held pin and Apply across inline rename passed"
}
function Custom-Options {
    (New-WorkspaceButton).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Custom fixture did not open naming'
    (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Grip options custom workspace')
    Wait-Until {(Control 'Create and Switch' -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)).Current.IsEnabled} 'Custom workspace name did not enable confirmation'
    Choose 'Create and Switch';Settled
    Wait-Until {$null -eq (Manager).prompt -and (Manager).page -eq 'workspaces'} 'New workspace closed the manager'
    $custom=(Storage).id
    Write-Output "$Device New retains manager passed"
    if((Manager).selected -ne $custom){throw 'New workspace was not selected in retained manager'}
    (Control ('workspace-manager-row-'+$active)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $active -and !(Manager).loading} 'Original workspace selection did not settle'
    Settled;Choose (Manager).primary;Closed
    Wait-Until {(Storage).id -eq $active -and !(Storage).busy} 'Original workspace did not return'
    Open-Manager;Preview-Select
    $saved=@((Storage).order);$revision=(Storage).switcher_revision
    $nameGeometry=Workspace-TextGeometry $custom
    Capture 'workspace-rename-display' -Composed
    Row-Context $custom
    Preview-Intact;Order-Intact $saved $revision
    Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'Custom context Rename did not open inline'
    $editor=Control ('workspace-manager-rename-'+$custom) -Arranged;$editor.SetFocus()
    $editingGeometry=Workspace-TextGeometry $custom -Editing
    @{before=$nameGeometry;editing=$editingGeometry}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'rename-geometry.json')
    Assert-WorkspaceTextGeometry $nameGeometry $editingGeometry
    Capture 'workspace-rename-input' -Composed
    Assert-WorkspaceRenderedName $nameGeometry $editingGeometry (Join-Path $run 'workspace-rename-display.png') (Join-Path $run 'workspace-rename-input.png')
    $editor.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Discarded inline name')
    [CapyRowPointer]::Key([uint32]$review.Id,27)
    Wait-Until {$null -eq (Manager).prompt} 'Escape did not cancel inline Rename'
    if(@((Manager).rows|Where-Object id -eq $custom)[0].title -ne 'Grip options custom workspace'){throw 'Escape persisted inline name'}
    Preview-Intact;Order-Intact $saved $revision
    Write-Output "$Device context Rename Escape passed"
    $previousPreview=$preview
    $bounds=(Control ('workspace-manager-row-'+$custom) -Arranged).Current.BoundingRectangle
    [CapyRowPointer]::DoubleClick([int]($bounds.X+$bounds.Width*.3),[int]($bounds.Y+$bounds.Height/2))|Out-Null
    Wait-Until {(Manager).prompt_action.type -eq 'rename' -and !(Manager).loading} 'Double-click name did not open inline Rename'
    if((Manager).selected -ne $custom){throw 'Double-click first click did not preview its row'}
    $script:preview=$custom;$script:previewLayout=Layout
    $editor=Control ('workspace-manager-rename-'+$custom) -Arranged;$editor.SetFocus()
    $editor.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Renamed workspace')
    [CapyRowPointer]::Key([uint32]$review.Id,13)
    Wait-Until {$null -eq (Manager).prompt -and @((Manager).rows|Where-Object id -eq $custom)[0].title -eq 'Renamed workspace'} 'Inline Enter did not commit'
    Preview-Intact;Order-Intact $saved $revision
    Write-Output "$Device double-click Rename Enter passed"
    $script:preview=$previousPreview;Preview-Select
    Row-Context $custom;Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'Focus-loss rename did not open'
    $editor=Control ('workspace-manager-rename-'+$custom) -Arranged;$editor.SetFocus()
    $editor.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('Focus committed workspace')
    [CapyRowPointer]::Key([uint32]$review.Id,9)
    Wait-Until {$null -eq (Manager).prompt -and @((Manager).rows|Where-Object id -eq $custom)[0].title -eq 'Focus committed workspace'} 'Inline focus loss did not commit'
    Preview-Intact;Order-Intact $saved $revision
    Write-Output "$Device inline Rename Tab commit passed"
    if($Device -eq 'mouse'){Rejected-InlineNameAdmission $custom}
    Held-Rename $custom
    $revision=(Storage).switcher_revision
    Row-Context $custom;Invoke 'workspace-manager-delete'
    Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Context Delete did not open'
    Choose 'Cancel';Settled
    Wait-Until {$null -eq (Manager).prompt} 'Delete Cancel did not return to manager'
    Preview-Intact;Order-Intact $saved $revision
    if($Device -eq 'pen'){
        $footerPreview=$preview;$script:preview=$custom;Preview-Select
        $remove=Control 'workspace-manager-remove' -Arranged
        Wait-Until {$remove.Current.IsEnabled} 'Selected custom workspace did not enable footer Delete'
        $remove.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
        Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Pen footer Delete did not open'
        Choose 'Cancel';Settled
        Wait-Until {$null -eq (Manager).prompt} 'Pen footer Delete Cancel did not return to manager'
        $script:preview=$footerPreview;Preview-Select
        Preview-Intact;Order-Intact $saved $revision
    }
    if($Device -in @('touch','pen')){
        Swipe-Reveal $custom
        Preview-Intact;Order-Intact $saved $revision
        Capture ($Device+'-swipe-before-outside') -Composed
        $null=Native-TapAt (New-WorkspaceButton) 0
        Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Outside press swallowed the New workspace action'
        Choose 'Cancel';Settled
        Wait-Until {$null -eq (Manager).prompt} 'Outside New Cancel did not return to manager'
        Preview-Intact;Order-Intact $saved $revision
        Capture ($Device+'-swipe-outside-dismissed') -Composed
        Swipe-Reveal $custom
        Preview-Intact;Order-Intact $saved $revision
        Capture ($Device+'-swipe-delete') -Composed
        Swipe-DeleteAction $custom
    }else{
        $previousPreview=$preview;$script:preview=$custom;Preview-Select
        $remove=Control 'workspace-manager-remove' -Arranged
        Wait-Until {$remove.Current.IsEnabled} 'Selected custom workspace did not enable footer Delete'
        $remove.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    }
    Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Custom cleanup did not open Delete'
    Choose 'Delete';Settled
    Wait-Until {@((Manager).rows|Where-Object id -eq $custom).Count -eq 0 -and (Manager).page -eq 'workspaces'} 'Delete closed manager or retained removed workspace'
    if($Device -notin @('touch','pen')){$script:preview=$previousPreview;Preview-Select}
    Preview-Intact
    Capture ($Device+'-custom-context-inline-delete') -Composed
}

function Prepare-MiddleRowIdle {
    $selected=(Manager).selected;$pins=@((Storage).switcher.id);$saved=@((Storage).order);$revision=(Storage).switcher_revision
    if($selected -ne $preview){throw 'Middle row preparation lost the accepted preview'}
    $trace=Native-TapAt (Control ('workspace-manager-row-'+$selected) -Arranged) 0
    Wait-Until {
        $g=Gesture
        $g -and $g.phase -eq 'idle' -and !$g.source -and !$g.can_drop -and !$g.panning -and $g.last_release.source -eq $selected -and !$g.last_release.commit -and !$g.last_release.canceled
    } 'Ordinary selected-row tap did not publish its real idle release'
    Preview-Intact;Order-Intact $saved $revision
    if(((Storage).switcher.id -join '|') -cne ($pins -join '|') -or (Find 'workspace-manager-move-up')){throw 'Middle row preparation changed pins or opened a grip menu'}
    $trace.gesture_after=Gesture;$trace.passed=$true
    $trace|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'middle-selected-row-preparation.json')
}

function Prove-MiddleRowHit([string]$Id,[string]$ControlId,$At,[int]$Index,[switch]$Grip){
    $expectedPreview=$preview;$expectedLayout=$previewLayout;$pins=@((Storage).switcher.id);$saved=@((Storage).order);$revision=(Storage).switcher_revision
    $item=Control $ControlId -Arranged;$bounds=$item.Current.BoundingRectangle;$runtime=@($item.GetRuntimeId());$before=Gesture
    $viewport=(Control 'workspace-manager-items').Current.BoundingRectangle
    $point=[CapyRowPointer+Point]::new();$point.x=$At.x;$point.y=$At.y;$hwnd=[CapyRowPointer]::WindowFromPoint($point);$owner=[uint32]0
    [CapyRowPointer]::GetWindowThreadProcessId($hwnd,[ref]$owner)|Out-Null
    if($item.Current.IsOffscreen -or $item.Current.ProcessId -ne $review.Id -or !$bounds.Contains($At.x,$At.y) -or !$viewport.Contains($At.x,$At.y) -or $owner -ne $review.Id -or [CapyRowPointer]::GetForegroundWindow() -ne $drawingWindow -or !$item.Current.BoundingRectangle.Equals($bounds)){throw 'Exact routed point lacks its stable visible owned native target'}
    if(!$before -or $before.phase -ne 'idle' -or $before.source -or $before.can_drop -or $before.panning -or (Find 'workspace-manager-move-up')){throw 'Routed Middle proof inherited contact or menu'}
    $generation=[double]$before.last_release.generation
    $tap=Native-TapAt $item 0 -At $At
    Wait-Until {
        $g=Gesture
        $g -and $g.phase -eq 'idle' -and !$g.source -and !$g.can_drop -and !$g.panning -and $g.last_release.generation -gt $generation -and $g.last_release.source -eq $Id -and !$g.last_release.commit -and !$g.last_release.canceled
    } 'Exact Middle point did not route a fresh ordinary release to its row'
    $release=Gesture;$expectedSelected=if($Grip){$expectedPreview}else{$Id}
    Wait-Until {(Manager).selected -eq $expectedSelected -and !(Manager).loading -and (Control ('workspace-manager-row-'+$expectedSelected)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected} 'Routed Middle point did not retain its expected native selection'
    if($Grip){
        $menu=Control 'workspace-manager-move-up' -Arranged
        if(!$menu.Current.IsEnabled){$menu=Control 'workspace-manager-move-down' -Arranged}
        if($menu.Current.ProcessId -ne $review.Id){throw 'Routed grip menu is foreign'}
        $menu.SetFocus();Wait-Until {$menu.Current.HasKeyboardFocus} 'Routed grip menu did not acquire keyboard focus'
        [CapyRowPointer]::Key([uint32]$review.Id,27)
        Wait-Until {!(Find 'workspace-manager-move-up')} 'Routed grip menu did not dismiss'
    }elseif((Find 'workspace-manager-move-up') -or (Find 'workspace-manager-rename')){throw 'Routed body proof opened a native menu'}
    try{
        Preview-Select
        if($previewLayout -cne $expectedLayout){throw 'Routed hit restoration changed the accepted preview layout'}
    }finally{$script:previewLayout=$expectedLayout}
    Preview-Intact;Order-Intact $saved $revision
    if(((Storage).switcher.id -join '|') -cne ($pins -join '|')){throw 'Routed hit proof changed workspace pins'}
    $after=Control $ControlId -Arranged;$idle=Gesture
    if($after.Current.ProcessId -ne $review.Id -or ($after.GetRuntimeId()-join',') -cne ($runtime-join',') -or !$after.Current.BoundingRectangle.Equals($bounds) -or !$bounds.Contains($At.x,$At.y)){throw 'Routed target identity or exact bounds changed before Middle input'}
    if($idle.phase -ne 'idle' -or $idle.source -or $idle.can_drop -or $idle.panning -or (Find 'workspace-manager-move-up') -or !(Control ('workspace-manager-row-'+$expectedPreview)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Current.IsSelected){throw 'Routed proof did not restore native preview and idle'}
    $proof=@{id=$ControlId;row_id=$Id;point=@($At.x,$At.y);runtime_id=$runtime;bounds=$bounds.ToString();native_release=$release;restored_preview=$expectedPreview;restored_layout=$expectedLayout;order=$saved;revision=$revision;pins=$pins;tap=$tap;grip=[bool]$Grip;passed=$true}
    $proof|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run ('middle-routed-hit-'+$script:gestureCase+'-'+$Index+'.json'))
    $proof
}

function Middle-RowDrag([switch]$Grip){
    $script:gestureCase=if($Grip){'middle-grip-rejected'}else{'middle-body-rejected'}
    $saved=@((Storage).order);$revision=(Storage).switcher_revision;$source=Source-Id
    $from=Point $source -Grip:$Grip;$to=Point $saved[0] -Top
    $sourceControlId=$(if($Grip){'workspace-manager-grip-'}else{'workspace-manager-row-'})+$source
    $targetControlId='workspace-manager-row-'+$saved[0];$ids=@($sourceControlId,$targetControlId)
    $routed=@((Prove-MiddleRowHit $source $sourceControlId $from 0 -Grip:$Grip),(Prove-MiddleRowHit $saved[0] $targetControlId $to 1))
    $hits=@(for($i=0;$i -lt 2;$i++){
        $at=@($from,$to)[$i];$item=Control $ids[$i] -Arranged;$bounds=$item.Current.BoundingRectangle
        $viewport=(Control 'workspace-manager-items').Current.BoundingRectangle
        $native=[CapyRowPointer+Point]::new();$native.x=$at.x;$native.y=$at.y;$hwnd=[CapyRowPointer]::WindowFromPoint($native);$owner=[uint32]0
        [CapyRowPointer]::GetWindowThreadProcessId($hwnd,[ref]$owner)|Out-Null
        $hit=Input-Trace $at.x $at.y
        $failedCheck=if($item.Current.IsOffscreen){'visible'}elseif(!$bounds.Contains($at.x,$at.y)){'point_in_row'}elseif(!$viewport.Contains($at.x,$at.y)){'point_in_viewport'}elseif($owner -ne $review.Id){'owned_hwnd'}elseif($hit.foreground -ne $drawingWindow.ToInt64()){'owned_foreground'}elseif(!$routed[$i].passed -or $routed[$i].id -cne $ids[$i] -or ($routed[$i].runtime_id -join ',') -cne ($item.GetRuntimeId() -join ',') -or $routed[$i].bounds -cne $bounds.ToString() -or ($routed[$i].point -join ',') -cne ((@($at.x,$at.y)) -join ',')){'owned_hit'}elseif(!$item.Current.BoundingRectangle.Equals($bounds)){'stable_bounds'}
        if($failedCheck){
            @{case=$script:gestureCase;index=$i;id=$ids[$i];failed_check=$failedCheck;point=@($at.x,$at.y);bounds=$bounds.ToString();viewport=$viewport.ToString();hwnd=$hwnd.ToInt64();owner=$owner;expected_owner=$review.Id;foreground=$hit.foreground;expected_foreground=$drawingWindow.ToInt64();hit=@($hit.hit|ForEach-Object {@{id=$_.id;name=$_.name;process=$_.process;offscreen=$_.offscreen;bounds=$_.bounds.ToString()}})}|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run ('middle-row-preflight-'+$script:gestureCase+'-'+$i+'.json'))
            throw 'Invalid run: middle row drag lacks its visible owned row or grip hit'
        }
        @{id=$ids[$i];point=$at;bounds=$bounds.ToString();runtime_id=@($item.GetRuntimeId());hwnd=$hwnd.ToInt64();hit=$hit;routed_hit=$routed[$i]}
    })
    $before=Gesture
    if($before.phase -ne 'idle' -or $before.source -or $before.can_drop -or $before.panning -or (Find 'workspace-manager-move-up')){throw 'Invalid run: middle row drag inherited an active contact or menu'}
    $trace=@{case=$script:gestureCase;device=$Device;button='middle';grip=[bool]$Grip;source=$source;from=$from;to=$to;hits=$hits;gesture_before=$before;order_before=$saved;revision_before=$revision;selected_before=(Manager).selected;passed=$false}
    try {
        $trace.down_called_utc=[DateTime]::UtcNow.ToString('o');$previous=0L;$requestGap=0.
        [CapyRowPointer]::Down('mouse',$from.x,$from.y,'middle');$previous=[Diagnostics.Stopwatch]::GetTimestamp()
        try {
            for($step=1;$step -le 12;$step++){
                [CapyRowPointer]::Move([int]($from.x+($to.x-$from.x)*$step/12),[int]($from.y+($to.y-$from.y)*$step/12))
                $now=[Diagnostics.Stopwatch]::GetTimestamp();$requestGap=[Math]::Max($requestGap,($now-$previous)*1000./[Diagnostics.Stopwatch]::Frequency);$previous=$now
                Start-Sleep -Milliseconds 15
            }
            [CapyRowPointer]::Up()
            $now=[Diagnostics.Stopwatch]::GetTimestamp();$requestGap=[Math]::Max($requestGap,($now-$previous)*1000./[Diagnostics.Stopwatch]::Frequency)
            $trace.up_returned_utc=[DateTime]::UtcNow.ToString('o');$trace.contact_ms=[CapyRowPointer]::ContactDurationMilliseconds;$trace.max_gap_ms=[CapyRowPointer]::MaxGapMilliseconds;$trace.request_max_gap_ms=$requestGap
        } finally {if([CapyRowPointer]::Active){[CapyRowPointer]::Cancel()}}
        Settled;Start-Sleep -Milliseconds 200
        Preview-Intact;Order-Intact $saved $revision
        $after=Gesture;$trace.gesture_after=$after;$trace.menu_after=$null -ne (Find 'workspace-manager-move-up');$trace.selected_after=(Manager).selected;$trace.order_after=@((Storage).order);$trace.revision_after=(Storage).switcher_revision
        if($after.phase -ne 'idle' -or $after.source -or $after.can_drop -or $after.panning -or $trace.menu_after -or ($after.last_release|ConvertTo-Json -Depth 8 -Compress) -ne ($before.last_release|ConvertTo-Json -Depth 8 -Compress) -or ($after.last_cancel|ConvertTo-Json -Depth 8 -Compress) -ne ($before.last_cancel|ConvertTo-Json -Depth 8 -Compress)){throw 'Middle-button row motion acquired a custom owner or changed the native menu'}
        $trace.passed=$true
    } finally {$secondaryContacts.Add($trace);$secondaryContacts|ConvertTo-Json -Depth 20|Set-Content (Join-Path $run 'secondary-contacts.json')}
    Write-Output ('mouse '+$script:gestureCase+' passed')
}

function No-Drop([switch]$Outside){
    $script:gestureCase=if($Outside){'outside'}else{'no-op'}
    $source=Source-Id;$saved=@((Storage).order);$revision=(Storage).switcher_revision
    if($Outside){
        $bounds=(Control 'workspace-manager-items').Current.BoundingRectangle
        $at=@{x=[int]($bounds.X+$bounds.Width*.4);y=[int]($bounds.Y-12)}
    }else{$at=Point $source}
    Press $source -Grip;Start-Sleep -Milliseconds 45;Move-To $at
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and !$g.can_drop} 'No-op or outside drop was offered as a reorder'
    $hit=Gesture
    if($hit.source -ne $source -or $hit.device -ne $Device -or !$hit.grip){throw 'No-op fixture hit the wrong source or device'}
    [CapyRowPointer]::Up()
    Wait-Until {(Gesture).phase -eq 'idle'} 'Rejected drop did not release its state'
    Preview-Intact;Order-Intact $saved $revision
    if(Find 'workspace-manager-move-up'){throw 'Rejected grip drop opened its click menu'}
}

function Drag([switch]$Grip,[switch]$Hold,[switch]$Cancel){
    $source=Source-Id;$saved=@((Storage).order);$revision=(Storage).switcher_revision
    $target=Point $saved[0] -Top
    Press $source -Grip:$Grip
    if($Hold){
        Wait-Until {(Gesture).phase -eq 'held'} "$Device hold was not recognized" 4
        if(!(Find 'workspace-manager-move-up')){throw 'Hold did not open a native context menu'}
    }else{Start-Sleep -Milliseconds 45}
    Preview-Intact;Order-Intact $saved $revision
    Move-To $target
    Wait-Until {$g=Gesture;$g.phase -eq 'dragging' -and $g.can_drop} "$Device did not drag with the original contact"
    $gesture=Gesture
    if($gesture.device -ne $Device -or $gesture.grip -ne [bool]$Grip -or $gesture.source -ne $source){throw 'Native pointer device or actual row hit differs from the fixture'}
    if(Find 'workspace-manager-move-up'){throw 'Dragging left the context menu open'}
    Preview-Intact;Order-Intact $saved $revision
    if($Cancel){
        $previousRelease=(Gesture).last_release.generation
        $previousCancel=(Gesture).last_cancel.generation
        if($Device -eq 'mouse'){[CapyRowPointer]::Key(27);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
        Wait-Until {(Gesture).phase -eq 'idle'} 'Canceled drag did not release its state'
        $terminal=(Gesture).last_release
        if($terminal.generation -ne $previousRelease -and $terminal.commit){
            $terminal|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run 'canceled-drop.json')
            throw 'Canceled pointer was submitted as a drop'
        }
        $cancellation=(Gesture).last_cancel
        $expectedReasons=if($Device -eq 'mouse'){@('escape')}else{@('pointer_canceled','capture_lost')}
        if(!$cancellation -or $cancellation.generation -eq $previousCancel -or $cancellation.reason -notin $expectedReasons){
            throw 'The fixture did not deliver the expected native cancellation'
        }
        $cancellation|ConvertTo-Json|Set-Content (Join-Path $run 'cancellation.json')
        Preview-Intact;Order-Intact $saved $revision
    }else{
        [CapyRowPointer]::Up()
        Wait-Until {(Storage).switcher_revision -gt $revision} 'Drop was not persisted'
        Settled;Preview-Intact
        if((Storage).switcher_revision -ne $revision+1 -or (Storage).order[0] -ne $source){throw 'Drop did not commit one reorder'}
    }
    if(Find 'workspace-manager-move-up'){throw 'Finished drag opened the grip click menu'}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch $Device
    [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if($scale -le 0 -or [double]::IsNaN($scale) -or [double]::IsInfinity($scale)){throw 'Invalid run: owned window DPI is unavailable'}
    if(![CapyRowPointer]::SetForegroundWindow($drawingWindow)){throw 'Could not focus owned input review'}
    $active=(Storage).id
    $preview=@((Storage).order|Where-Object {$_ -ne $active})[0]
    Open-Manager;Preview-Select
    if(!(Find ('workspace-manager-current-'+$active))){throw 'Current workspace checkmark is missing'}
    $wrong=Find ('workspace-manager-current-'+$preview)
    if($wrong -and !$wrong.Current.IsOffscreen){throw 'Dialog preview moved the current workspace checkmark'}
    $null=New-WorkspaceButton
    $taps=[Collections.Generic.List[object]]::new()
    $secondaryContacts=[Collections.Generic.List[object]]::new()
    if($Device -eq 'mouse'){Prepare-MiddleRowIdle;Middle-RowDrag;Middle-RowDrag -Grip}
    $bodyDrifts=if($Device -eq 'mouse'){@(0,1)}else{@(0,1,3,6)}
    foreach($drift in $bodyDrifts){
      foreach($moveSettle in $(if($drift -eq 6){@(0,35)}else{@(0)})){
        $source=Source-Id;$item=Control ('workspace-manager-row-'+$source) -Arranged;$list=Control 'workspace-manager-items';$scroll=$null
        $hasScroll=$list.TryGetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern,[ref]$scroll)
        $beforeScroll=if($hasScroll){$scroll.Current.VerticalScrollPercent}else{$null}
        $trace=Native-TapAt $item $drift -Vertical:($drift -eq 6) -Fraction .4 -MoveSettleMilliseconds $moveSettle;$trace.case='row-body-'+$drift+$(if($moveSettle){'-settled'}else{''});$trace.expected=$source;$trace.scroll_before=$beforeScroll
        try{Wait-Until {(Manager).selected -eq $source -and !(Manager).loading} "$Device row-body $drift DIP tap no longer previews";$trace.passed=$true}
        finally{$trace.actual=(Manager).selected;$trace.gesture_final=Gesture;$trace.scroll_after=if($hasScroll){$scroll.Current.VerticalScrollPercent}else{$null};$taps.Add($trace);$taps|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'native-taps.json')}
        Preview-Select
      }
    }
    Write-Output "$Device short click passed"
    foreach($row in (Manager).rows){if(Find ('workspace-manager-options-'+$row.id) -Visible){throw 'Included workspace still has a row options button'}}
    foreach($drift in @(0,1)){
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        $source=Source-Id;$trace=Native-TapAt (Control ('workspace-manager-grip-'+$source) -Arranged) $drift;$trace.case='grip-menu-'+$drift
        try{Wait-Until {$null -ne (Find 'workspace-manager-move-up')} "$Device grip $drift DIP tap did not open its menu";Preview-Intact;Order-Intact $saved $revision;if((Find 'workspace-manager-rename') -or (Find 'workspace-manager-delete')){throw 'Grip menu contains protected row actions'};$trace.passed=$true}
        finally{$trace.gesture_final=Gesture;$taps.Add($trace);$taps|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'native-taps.json')}
        if($drift -eq 0){Capture ($Device+'-grip-menu') -Composed}
        [CapyRowPointer]::Key(27);Wait-Until {!(Find 'workspace-manager-move-up')} 'Acknowledged grip menu did not dismiss'
    }
    if($Device -ne 'mouse'){
        $script:gestureCase='motion-before-hold-outside'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision;$source=Source-Id;$at=Point $source
        $listBounds=(Control 'workspace-manager-items').Current.BoundingRectangle;$hostBounds=$root.Current.BoundingRectangle
        $outside=@{x=[int]($listBounds.Left-12);y=$at.y}
        if($outside.x -le $hostBounds.Left -or $outside.x -ge $hostBounds.Right -or $outside.y -le $hostBounds.Top -or $outside.y -ge $hostBounds.Bottom -or $listBounds.Contains($outside.x,$outside.y)){throw 'Invalid run: no blank owned list surround for outside release'}
        Press $source;Start-Sleep -Milliseconds 45
        Move-To $outside
        [CapyRowPointer]::Up()
        Wait-Until {!(Gesture).panning} 'Native panning did not settle before the next gesture'
        @{source=$source;from=$at;to=$outside;list_bounds=$listBounds.ToString();host_bounds=$hostBounds.ToString();gesture=(Gesture);selected=(Manager).selected}|ConvertTo-Json -Depth 10|Set-Content (Join-Path $run 'outside-contact.json')
        Preview-Intact;Order-Intact $saved $revision
        if(Find 'workspace-manager-move-up'){throw 'Motion before hold opened a context menu'}
        Write-Output "$Device motion before hold passed"
        Press $source
        Wait-Until {(Gesture).phase -eq 'held'} "$Device hold was not recognized" 4
        [CapyRowPointer]::Up()
        Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Held release did not retain its menu'
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Key(27)
        Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Escape did not dismiss the held menu'
        if(!(Manager)){throw 'Dismissing the menu also closed the manager'}
        Write-Output "$Device held menu release passed"
    }else{
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        Press (Source-Id);Start-Sleep -Milliseconds 1100
        if(Find 'workspace-manager-move-up'){throw 'Mouse hold opened a menu'}
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Up()
        Wait-Until {!(Manager).loading} 'Mouse click did not settle'
        Preview-Select
    }
    Drag -Grip
    Write-Output "$Device immediate grip passed"
    Drag -Hold:($Device -ne 'mouse')
    Write-Output "$Device body reorder passed"
    Drag -Grip -Cancel
    Write-Output "$Device cancellation passed"
    No-Drop;No-Drop -Outside
    Write-Output "$Device no-op and outside drops passed"
    foreach($key in @(13,32)){
        $source=$preview;$saved=@((Storage).order);$revision=(Storage).switcher_revision
        $control=Control ('workspace-manager-grip-'+$source)
        Focus-Keyboard $control
        $trace=@{case='grip-key-'+$key;key=$key;source=$source;focus_before=@(Control-Trace ([System.Windows.Automation.AutomationElement]::FocusedElement));passed=$false}
        try{
            [CapyRowPointer]::Key([uint32]$review.Id,[uint16]$key)
            Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Keyboard grip activation did not open the move menu'
            Preview-Intact;Order-Intact $saved $revision
            if((Find 'workspace-manager-rename') -or (Find 'workspace-manager-delete')){throw 'Keyboard grip menu contains protected row actions'}
            $trace.passed=$true
        }finally{
            $trace.gesture_final=Gesture;$trace.focus_after=@(Control-Trace ([System.Windows.Automation.AutomationElement]::FocusedElement))
            $taps.Add($trace);$taps|ConvertTo-Json -Depth 15|Set-Content (Join-Path $run 'native-taps.json')
        }
        [CapyRowPointer]::Key(27)
        Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Keyboard grip menu did not dismiss'
    }
    $source=Source-Id;$revision=(Storage).switcher_revision
    Invoke ('workspace-manager-grip-'+$source)
    Invoke 'workspace-manager-move-up'
    Wait-Until {(Storage).switcher_revision -gt $revision} 'Accessible grip command did not persist'
    Settled;Preview-Intact
    (Control ('workspace-manager-row-'+$preview)).SetFocus()
    [CapyRowPointer]::Key(38)
    Wait-Until {(Manager).selected -ne $preview -and !(Manager).loading} 'Keyboard navigation no longer previews'
    [CapyRowPointer]::Key(13)
    if(!(Manager)){throw 'Enter unexpectedly applied and closed the manager'}
    Preview-Select
    Write-Output "$Device accessible grip and keyboard preview passed"
    $at=Point (Source-Id)
    [CapyRowPointer]::RightClick($at.x,$at.y)
    Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Secondary click did not open the row menu'
    Preview-Intact
    [CapyRowPointer]::Key(27)
    Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Secondary menu did not dismiss'
    (Control ('workspace-manager-row-'+$preview)).SetFocus()
    [CapyRowPointer]::Key(93)
    Wait-Until {$null -ne (Find 'workspace-manager-move-up')} 'Keyboard menu action did not open the row menu'
    Preview-Intact
    [CapyRowPointer]::Key(27)
    Wait-Until {$null -eq (Find 'workspace-manager-move-up')} 'Keyboard menu did not dismiss'
    Write-Output "$Device secondary and keyboard menus passed"
    Pin-Buttons
    Custom-Options
    Capture ($Device+'-rows') -Composed
    Choose 'Cancel';Closed
    Open-Manager;Preview-Select
    foreach($row in (Manager).rows){if(Find ('workspace-manager-options-'+$row.id) -Visible){throw 'Reopened included workspace still has a row options button'}}
    (Control ('workspace-manager-row-'+$preview)).SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,27);Closed
    if((Storage).id -ne $active){throw 'Cancel changed the active workspace'}

    if($Overflow){
        for($i=1;$i -le 6;$i++){
            Open-Manager;Invoke 'workspace-manager-create'
            Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Creation prompt missing'
            (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue("Row overflow workspace $i")
            Choose 'Create and Switch';Settled
            Wait-Until {$null -eq (Manager).prompt -and (Manager).page -eq 'workspaces'} 'Creation closed manager'
            Choose 'Cancel';Closed
            Wait-Until {$s=Storage;$s.ready -and !$s.busy -and !$s.switcher_busy} 'Created workspace did not settle'
        }
        $active=(Storage).id
        Open-Manager;Preview-Select
        $scroller=(Control 'workspace-manager-items').GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
        Wait-Until {$scroller.Current.VerticallyScrollable} 'Rows did not overflow'
        $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
        Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not scroll to the beginning'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        if($Device -ne 'mouse'){
            $at=Point $saved[2]
            Press $saved[2];Start-Sleep -Milliseconds 45
            Move-To @{x=$at.x;y=$at.y-100}
            Start-Sleep -Milliseconds 45;[CapyRowPointer]::Up()
            Wait-Until {$scroller.Current.VerticalScrollPercent -gt 1 -and !(Gesture).panning} "$Device did not preserve ordinary row scrolling"
            Preview-Intact;Order-Intact $saved $revision
            $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
            Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not return to the beginning'
        }
        $source=$saved[0]
        $bounds=(Control 'workspace-manager-items').Current.BoundingRectangle
        Press $source -Grip;Start-Sleep -Milliseconds 45
        Move-To @{x=[int]($bounds.X+$bounds.Width*.4);y=[int]($bounds.Bottom-5)}
        Wait-Until {$scroller.Current.VerticalScrollPercent -ge 99 -and (Gesture).phase -eq 'dragging' -and (Gesture).can_drop} 'Drag did not retain its source while scrolling to the end'
        Preview-Intact;Order-Intact $saved $revision
        [CapyRowPointer]::Up()
        Wait-Until {(Storage).switcher_revision -gt $revision} 'Scrolled drop was not persisted'
        Settled;Preview-Intact
        if((Storage).order[-1] -ne $source -or (Storage).switcher_revision -ne $revision+1){throw 'Scrolled drop did not commit one move to the end'}
        Write-Output "$Device scroll overflow and retained drag passed"
        $scroller.SetScrollPercent([System.Windows.Automation.ScrollPattern]::NoScroll,0)
        Wait-Until {$scroller.Current.VerticalScrollPercent -le 1} 'Rows did not return to the beginning'
        $script:gestureCase='minimize'
        $saved=@((Storage).order);$revision=(Storage).switcher_revision
        $at=Point $saved[2]
        Press $saved[0] -Grip;Start-Sleep -Milliseconds 45;Move-To $at
        Wait-Until {(Gesture).phase -eq 'dragging'} 'Blur review did not begin dragging'
        $previousRelease=(Gesture).last_release.generation;$previousCancel=(Gesture).last_cancel.generation
        $window=$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern)
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Minimized)
        Wait-Until {(Gesture).phase -eq 'idle'} 'Losing the window did not cancel the drag'
        $terminal=Gesture
        $terminal|ConvertTo-Json -Depth 8|Set-Content (Join-Path $run 'blur-cancellation.json')
        if(($terminal.last_release.generation -ne $previousRelease -and $terminal.last_release.commit) -or
            !$terminal.last_cancel -or $terminal.last_cancel.generation -eq $previousCancel){throw 'Losing the window did not cancel before committing a drop'}
        [CapyRowPointer]::Cancel()
        $window.SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
        Wait-Until {$window.Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Normal} 'Review window did not restore'
        Settled;Preview-Intact;Order-Intact $saved $revision
        Write-Output "$Device minimize and blur cancellation passed"
        Capture ($Device+'-overflow')
        Choose 'Cancel';Closed
        if((Storage).id -ne $active){throw 'Overflow review changed the active workspace'}
    }
    Close
    @{device=$Device;theme=$Theme;short_click='passed';pickup_and_menu='passed';grip_and_body_drop='passed';preview_preserved='passed';
        single_reorder_commit='passed';grip_short_tap_and_keyboard_menu='passed';builtin_dots_absent='passed';custom_context_inline_rename_delete='passed';cancel_and_reopen='passed';cancellation='passed';no_op_and_outside_drops='passed';accessible_grip_and_keyboard='passed';secondary_and_keyboard_menus='passed';secondary_mouse_drags=@{cases=$secondaryContacts.Count;passed=@($secondaryContacts|Where-Object passed).Count};overflow_scroll_and_blur=[bool]$Overflow;zero_exit='passed';
        scope='OS-injected native input; physical digitizers and performance require separate acceptance'}|
        ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
    Get-Content (Join-Path $run 'results.json')
}catch{
    try{Capture 'failure' -Composed}catch{}
    try{@{case=$script:gestureCase;gesture=(Gesture);model=(Model)}|ConvertTo-Json -Depth 90|Set-Content (Join-Path $run 'failure-state.json')}catch{}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    throw
}finally{
    try{[CapyRowPointer]::Dispose()}catch{}
    if($review -and !$review.HasExited){if($drawingWindow){$null=[CapyWindowApi]::PostMessage($drawingWindow,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)};if(!$review.WaitForExit(10000)){$review.Kill();$review.WaitForExit()}}
    Exit-CapyEnvironment
}
