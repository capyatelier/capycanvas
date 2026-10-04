function Relationship-Layer([double]$Id){@((Model).state.layers|Where-Object id -eq $Id)[0]}
function Relationship-Select([double]$Id){
    Relationship-Reveal "layer-$Id-name";Invoke "layer-$Id-name"
    Wait-Until {$l=(Model).state.layer_tools.editing_layer;$l.id -eq $Id -and !$l.mask_selected} 'Layer selection did not publish'
}
function Relationship-Visibility([double]$Id,[bool]$Visible){
    Invoke "layer-$Id-visibility"
    Wait-Until {(Relationship-Layer $Id).visible -eq $Visible} 'Local visibility did not publish'
}
function Relationship-Undo{
    $revision=(Model).state.document_file.revision;Invoke 'Undo' -Name
    Wait-Until {(Model).state.document_file.revision -ne $revision} 'Undo did not publish'
}
function Relationship-Rows{
    @((Model).state.layers|ForEach-Object {[ordered]@{id=$_.id;depth=$_.depth;visible=$_.visible;alpha_locked=$_.alpha_locked;pass_through=$_.pass_through;relationship=$_.relationship}})|ConvertTo-Json -Depth 8 -Compress
}
function Relationship-Layers{
    if($item=Find 'layer-attachment' -Visible){return}
    foreach($id in @('drawer-tab-layers','panel-tab-layers','column-icon-layers')){
        if(Find $id -Visible){Invoke $id;break}
    }
    Wait-Until {$null -ne (Find 'layer-attachment' -Visible)} 'Layers did not become visible'
}
function Relationship-Insert([string]$Category,[string]$Label){
    $before=@((Model).state.layers.id)
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Filter'
    Expand ('menu-'+$Category.ToLower())
    (Control $Label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem) -Arranged).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -ne (Model).state.layer_tools.editing_layer -and $before -notcontains (Model).state.layer_tools.editing_layer.id} 'Filter did not insert'
    Relationship-Layers
    (Model).state.layer_tools.editing_layer.id
}
function Relationship-New([switch]$Group){
    $before=@((Model).state.layers.id);Invoke $(if($Group){'layer-new-group'}else{'layer-new'})
    Wait-Until {$before -notcontains (Model).state.layer_tools.editing_layer.id} 'Layer did not insert'
    (Model).state.layer_tools.editing_layer.id
}
function Relationship-Attach([double]$Id,[string]$Kind,[double]$Target){
    Toggle-Flag 'layer-attachment'
    Wait-Until {$r=(Relationship-Layer $Id).relationship;$r.kind -eq $Kind -and $r.target -eq $Target} 'Shared attachment action did not apply'
    $control=Control 'layer-attachment';$view=(Model).state.layer_tools.attachment
    if($control.Current.Name -ne $view.label -or $control.Current.HelpText -ne $view.description){throw 'Attachment caption and description differ from shared state'}
}
function Relationship-Reveal([string]$Id){
    if($Id -notmatch '^layer-(?:row-)?([0-9]+)(?:-|$)'){return}
    $ids=@((Model).state.layers.id);$index=[Array]::IndexOf($ids,[int64]$Matches[1])
    $list=Control 'layer-list';$scroll=$list.GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    if($index -lt 0 -or !$scroll.Current.VerticallyScrollable){return}
    for($attempt=0;$attempt -lt 12;$attempt++){
        $viewport=$list.Current.BoundingRectangle;$item=Find $Id -Visible
        if($item){
            $rect=$item.Current.BoundingRectangle
            if($rect.Top -ge $viewport.Top+60 -and $rect.Bottom -le $viewport.Bottom-60){return}
            $extent=$viewport.Height*(100-$scroll.Current.VerticalViewSize)/$scroll.Current.VerticalViewSize
            $percent=$scroll.Current.VerticalScrollPercent+100*($rect.Y+$rect.Height*.5-$viewport.Y-$viewport.Height*.5)/$extent
        }else{$percent=100.*$index/[Math]::Max(1,$ids.Count-1)}
        $percent=[Math]::Clamp($percent,0.,100.)
        $scroll.SetScrollPercent(-1,$percent);Start-Sleep -Milliseconds 150
        if($percent -eq 0 -or $percent -eq 100){if(Find $Id -Visible){return}}
    }
    throw "Layer target did not enter the native viewport: $Id"
}
function Relationship-Point([string]$Id,[double]$Fraction=.5){
    Relationship-Reveal $Id
    $item=Control $Id -Arranged;$r=$item.Current.BoundingRectangle
    @{x=[int]($r.X+$r.Width*.5);y=[int]($r.Y+$r.Height*$Fraction)}
}
function Relationship-Gesture{try{(Find 'layer-list').Current.ItemStatus|ConvertFrom-Json}catch{}}
function Relationship-Drop([double]$Id,[double]$Target,[string]$Surface,[double]$Fraction,[double]$Expected,[string]$Position,[switch]$Cancel,[switch]$Refused){
    $before=Relationship-Rows
    $from=Relationship-Point "layer-$Id-drag"
    [CapyRowPointer]::Down('mouse',$from.x,$from.y);Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Move($from.x+15,$from.y)
    Wait-Until {(Relationship-Gesture).phase -eq 'dragging'} 'Grip did not begin native dragging'
    $viewport=(Control 'layer-list').Current.BoundingRectangle
    [CapyRowPointer]::Move([int]($viewport.X+$viewport.Width*.5),[int]($viewport.Y+$viewport.Height*.5))
    $to=Relationship-Point $(if($Surface -eq 'thumbnail'){"layer-$Target-content"}else{"layer-row-$Target"}) $Fraction
    [CapyRowPointer]::Move($to.x,$to.y)
    if($Refused){
        Wait-Until {$g=Relationship-Gesture;$g.phase -eq 'dragging' -and $g.surface -eq $Surface} 'Refused drop did not retain its contact'
        [CapyRowPointer]::Up();Wait-Until {(Relationship-Gesture).phase -eq 'idle'} 'Refused drop did not finish shared validation'
        if((Relationship-Rows) -ne $before -or (Relationship-Gesture).last_release.commit){throw 'Pass Through thumbnail drop silently isolated the group'}
        return
    }
    try{Wait-Until {$g=Relationship-Gesture;$g.phase -eq 'dragging' -and $g.can_drop -and $g.target -eq $Expected -and $g.position -eq $Position -and $g.surface -eq $Surface} 'Native feedback did not use the shared normalized drop'}catch{
        @{gesture=(Relationship-Gesture);source=$Id;target=$Target;surface=$Surface;fraction=$Fraction;expected=$Expected;position=$Position;from=$from;to=$to}|ConvertTo-Json -Depth 12|Set-Content (Join-Path $run 'relationships-drop-failure.json')
        throw
    }
    if($Cancel){[CapyRowPointer]::Key(0x1b)}
    [CapyRowPointer]::Up()
    Wait-Until {(Relationship-Gesture).phase -eq 'idle'} 'Layer drop did not retire its contact'
    if($Cancel){if((Relationship-Rows) -ne $before){throw 'Canceled drop changed relationships'};return}
    Wait-Until {(Relationship-Rows) -ne $before} 'Layer drop did not change the shared model'
    $after=Relationship-Rows
    if(!(Relationship-Gesture).last_release.commit){throw 'Native release did not revalidate the drop'}
    Relationship-Undo;Wait-Until {(Relationship-Rows) -eq $before} 'Drop requires more than one Undo'
    Invoke 'Redo' -Name;Wait-Until {(Relationship-Rows) -eq $after} 'Redo did not restore normalized drop'
}
function Relationship-Swipe([double]$Id,[string]$Device,[int]$Distance,[switch]$Cancel,[string]$CaptureName){
    $from=Relationship-Point "layer-$Id-name"
    [CapyRowPointer]::Down($Device,$from.x,$from.y);Start-Sleep -Milliseconds 35
    [CapyRowPointer]::Move($from.x+$Distance,$from.y)
    if($CaptureName){Capture $CaptureName -Composed -WithModel}
    if($Cancel){[CapyRowPointer]::Cancel()}else{[CapyRowPointer]::Up()}
    Wait-Until {(Relationship-Gesture).phase -eq 'idle'} 'Right swipe did not retire its contact'
}
function Relationship-Theme{
    $before=(Model).state.theme;Invoke 'settings-button'
    $preferences=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control $(if($before -eq 'dark'){'Light'}else{'Dark'}) -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -ne $before} 'Theme did not publish'
    $close=$preferences.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'CloseButton'))
    $close.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {!(Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
    Relationship-Layers
}
function Test-LayerRelationships{
    $null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1780 -Height 1480
    try{
        $base=Relationship-New
        $baseFx=Relationship-Insert 'Blur' 'Gaussian Blur';Relationship-Attach $baseFx 'effect' $base
        Relationship-Select $base;$owner=Relationship-New;Relationship-Attach $owner 'clip' $base
        Invoke 'layer-add-mask';Wait-Until {(Relationship-Layer $owner).has_mask} 'Owner mask did not publish';Relationship-Select $owner
        $blur=Relationship-Insert 'Blur' 'Gaussian Blur';Relationship-Attach $blur 'effect' $owner
        $top=Relationship-Insert 'Tone' 'Curves';Relationship-Attach $top 'effect' $owner
        if((Relationship-Layer $top).has_thumbnail -or !(Relationship-Layer $top).adjustment_effect){throw 'Adjustment FX acquired a thumbnail'}
        Relationship-Visibility $blur $false;Relationship-Visibility $owner $false
        Wait-Until {(Relationship-Layer $top).visibility_blocked -and (Relationship-Layer $top).visible} 'Hidden owner did not block its locally visible effect'
        Capture 'relationships-hidden-owner' -Composed -WithModel
        Relationship-Visibility $owner $true
        Wait-Until {!(Relationship-Layer $top).visibility_blocked -and !(Relationship-Layer $blur).visible} 'Owner restoration lost local effect visibility'
        Relationship-Visibility $blur $true
        Wait-Until {(Relationship-Layer $blur).visible} 'Local effect visibility did not restore before pointer history'
        $before=Relationship-Rows;Capture 'relationships-stationary-rail-before' -Composed -WithModel
        Relationship-Swipe $top 'touch' -75 -Cancel -CaptureName 'relationships-stationary-rail-swipe'
        if((Relationship-Rows) -ne $before){throw 'Canceled effect swipe changed relationships'}
        foreach($device in @('touch','pen')){
            $before=Relationship-Rows
            Relationship-Swipe $base $device 12
            if((Relationship-Rows) -ne $before){throw 'Short right swipe changed the document'}
            Relationship-Swipe $base $device 75 -Cancel
            if((Relationship-Rows) -ne $before){throw 'Canceled right swipe changed the document'}
            Relationship-Swipe $base $device 75
            Wait-Until {(Relationship-Layer $base).alpha_locked} "$device right swipe did not use the shared paint action"
            Relationship-Undo;Wait-Until {(Relationship-Rows) -eq $before} 'Paint swipe requires more than one Undo'
        }
        $group=Relationship-New -Group
        Relationship-Select $group;Blend 'Pass Through'
        Wait-Until {(Relationship-Layer $group).pass_through} 'Pass Through did not publish before drop refusal'
        Relationship-Drop $top $group 'thumbnail' .5 -1 '' -Refused
        $nested=Relationship-New -Group;$child=Relationship-New
        Relationship-Select $group;Blend 'Multiply'
        Wait-Until {(Relationship-Layer $group).blend_label -eq 'Multiply' -and !(Relationship-Layer $group).pass_through} 'Isolated group blend did not publish'
        Relationship-Swipe $group 'touch' 75
        Wait-Until {(Relationship-Layer $group).pass_through} 'Group right swipe did not enter Pass Through'
        Capture 'relationships-pass-through' -Composed -WithModel
        Relationship-Swipe $group 'pen' 75
        Wait-Until {!(Relationship-Layer $group).pass_through -and (Relationship-Layer $group).blend_label -eq 'Multiply'} 'Group right swipe did not restore its retained blend'
        $groupFx=Relationship-Insert 'Tone' 'Exposure';Relationship-Attach $groupFx 'effect' $group
        if($null -ne (Relationship-Layer $group).right_swipe){throw 'A group effect owner admitted Pass Through'}
        Invoke "layer-$group-content"
        Wait-Until {$null -eq (Find "layer-$child-name")} 'Nested group child did not collapse'
        Capture 'relationships-collapsed-group' -Composed -WithModel
        Invoke "layer-$group-content"
        Wait-Until {$null -ne (Find "layer-$child-name")} 'Nested group child did not expand'
        Invoke 'layer-new-selection'
        Wait-Until {$null -ne (Model).state.layer_tools.rename_layer} 'Saved Selection did not begin naming'
        $saved=(Model).state.layer_tools.rename_layer
        Focus "layer-$saved-rename";[CapyRowPointer]::Key([uint32]$review.Id,0x0d)
        Wait-Until {$null -eq (Model).state.layer_tools.rename_layer} 'Saved Selection name did not finish'
        if((Control "layer-$saved-load").Current.Name -ne (Relationship-Layer $saved).load_selection_tooltip){throw 'Use Selection lost shared accessible copy'}
        Relationship-Select $top
        Relationship-Drop $saved $owner 'row' .125 $top 'above'
        $ids=@((Model).state.layers.id);$at=[Array]::IndexOf($ids,$saved)
        if($ids[$at+1] -ne $top -or $ids[$at+2] -ne $blur -or $ids[$at+3] -ne $owner){throw 'Saved Selection split the effect chain'}
        Relationship-Drop $top $blur 'row' .875 $owner 'above'
        Relationship-Undo
        $extra=Relationship-Insert 'Tone' 'Exposure'
        Relationship-Drop $extra $group 'thumbnail' .5 $group 'attach'
        if((Relationship-Layer $extra).relationship.target -ne $group){throw 'Group thumbnail drop did not attach to the group output'}
        Relationship-Undo
        Relationship-Drop $extra $group 'row' .5 $group 'into'
        if((Relationship-Layer $extra).relationship -or (Relationship-Layer $extra).depth -ne 1){throw 'Group row drop did not insert into the folder'}
        Relationship-Undo
        Relationship-Drop $extra $group 'thumbnail' .5 $group 'attach' -Cancel
        Relationship-Select $top;Capture 'relationships-default-theme' -Composed -WithModel
        $revision=(Model).state.document_file.revision;Relationship-Theme
        if((Model).state.document_file.revision -ne $revision){throw 'Theme publication changed artwork history'}
        Relationship-Select $top;Capture 'relationships-alternate-theme' -Composed -WithModel
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1100 -Height 780
        Relationship-Layers;Capture 'relationships-narrow' -Composed -WithModel
        Toggle-Flag 'layer-attachment';Wait-Until {$null -eq (Relationship-Layer $top).relationship} 'Effect release did not restore stack behavior'
        Relationship-Undo;Wait-Until {(Relationship-Layer $top).relationship.target -eq $owner} 'One Undo did not restore effect ownership'
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1780 -Height 1480
        Relationship-Layers
        foreach($i in 1..48){$null=Relationship-New}
        $scroll=(Control 'layer-list').GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
        $scroll.SetScrollPercent(-1,100)
        Wait-Until {$null -ne (Find "layer-$base-name" -Visible)} 'Scroll did not realize the relationship base'
        Capture 'relationships-scrolled' -Composed -WithModel
        $realized=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -match '^layer-[0-9]+-name$'})
        if($realized.Count -ge 40){throw 'Connections materialized offscreen rows'}
    }finally{[CapyRowPointer]::Dispose()}
}
