function Shown-Value($Control){$name=$Control.Current.Name;$name.Substring($name.LastIndexOf(" ")+1)}
function Workspace-Grips{
 $nodes=@($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -match '^(group-grip-|panel-footer-grip-|ribbon-grip-|divider-|floating-\d+-)'})
 $ids=@($nodes|ForEach-Object {$_.Current.AutomationId}|Select-Object -Unique)
 if(!@($ids|Where-Object {$_ -like 'group-grip-*'}).Count -or !@($ids|Where-Object {$_ -like 'divider-*' -or $_ -like 'floating-*'}).Count){throw 'Private workspace did not expose actual group and resize handles for retained-copy acceptance'}
 $script:gripIdentities=Native-Identities $ids
 @($ids)|ConvertTo-Json|Set-Content (Join-Path $run 'workspace-grip-inventory.json')
}
function Check-WorkspaceGrips($View,[string]$Tag){
 Check-Identities $script:gripIdentities
 foreach($id in $script:gripIdentities.Keys){
  $control=Control $id
  if($id -like 'group-grip-*'){
   $caption=Catalog-Text $Tag 'documents-delivery-move-group' 'documents'
   if($control.Current.HelpText -ne (Catalog-Text $Tag 'documents-delivery-drag-panel' 'documents')){throw 'Retained group handle help stayed in an earlier language'}
  }elseif($id -like 'panel-footer-grip-*' -or $id -like 'ribbon-grip-*'){
   $panelId=$id -replace '^(panel-footer-grip-|ribbon-grip-)','';$panel=@($View.panels|Where-Object id -eq $panelId)
   if($panel.Count -ne 1){throw 'Retained panel handle lost its semantic owner'}
   $caption=(Catalog-Text $Tag 'native-move-panel' 'common').Replace('{ $title }',$panel[0].title)
  }else{$caption=Catalog-Text $Tag 'documents-delivery-resize-dock' 'documents'}
  if($control.Current.Name -ne $caption){throw "Retained workspace handle caption stayed stale: $id"}
 }
}
function Numeric-Tooltip{
 $condition=[System.Windows.Automation.AndCondition]::new(
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ToolTip),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Preference-Numeric{
 Use-Window $first
 if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{Menu-Command 'edit' 'settings'}
 Invoke-Id 'preference-page-canvas'
 $entry=Control 'setting-number-pan_speed' -Arranged;$identity=$entry.GetRuntimeId() -join ':'
 $pan=(Read-Snapshot (Settings-File)).pan_speed
 $baseline=Fresh-Model;$document=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=$baseline.windows_gpu_generation
 Select-Draft $entry $numericDraft;Key 13;Select-Draft $entry $numericDraft
 Surface-Languages 'retained-numeric-validation' {
  $view=Fresh-Model $first $choice.tag;$control=Control 'setting-number-pan_speed' -Arranged
  if(($control.GetRuntimeId() -join ':') -ne $identity -or (Value $control) -ne $numericDraft -or (Selection $control) -ne $numericDraft -or ($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $view.windows_gpu_generation -ne $gpu -or (Read-Snapshot (Settings-File)).pan_speed -ne $pan){throw 'Native invalid numeric draft changed its editor, selected literal, setting or document/GPU ownership'}
  if($control.Current.HelpText -ne (Catalog-Text $choice.tag 'numeric-invalid-expression' 'common')){throw 'Retained typed numeric error accessibility help stayed in an earlier language'}
  $away=(Control 'preferences-heading').Current.BoundingRectangle;[CapyRowPointer]::Hover([int]($away.Left+$away.Width/2),[int]($away.Top+$away.Height/2))
  Wait-Until {!(Numeric-Tooltip)} 'Previous native numeric tooltip did not dismiss' 10
  $bounds=$control.Current.BoundingRectangle;[CapyRowPointer]::Hover([int]($bounds.Left+$bounds.Width/2),[int]($bounds.Top+$bounds.Height/2))
  Wait-Until {$tip=Numeric-Tooltip;$tip -and $tip.Current.Name -eq (Catalog-Text $choice.tag 'numeric-invalid-expression' 'common')} 'Retained typed numeric error tooltip did not use the current canonical copy' 10
 }
 Use-Window $first;$entry=Control 'setting-number-pan_speed';$entry.SetFocus();Key 27
 Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.preferences} 'Numeric Preferences did not close' 15
 Use-Window $textWindow
}

function Property-Panel([string]$Id){
 $view=Fresh-Model
 if(@($view.layout.groups|Where-Object active -eq $Id).Count -eq 0){Invoke-Id $(if(Find ('drawer-tab-'+$Id)){'drawer-tab-'+$Id}else{'panel-tab-'+$Id})}
 Wait-Until {$view=Model;$view -and @($view.layout.groups|Where-Object active -eq $Id).Count} "Native panel did not become active: $Id" 20
}
function Filter-Options{
 $box=Control 'filter-category';$pattern=$box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern);$pattern.Expand()
 $hit=@{items=@()};$count=@((Fresh-Model).state.filter_categories).Count
 Wait-Until {$hit.items=@($box.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::ListItem)));$hit.items.Count -eq $count} 'Filter category native option inventory did not match the shared categories' 15
 $pattern.Collapse();$hit.items
}
function Filter-Owner($View){
 if(($View.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $filterDocument -or $View.windows_gpu_generation -ne $filterGpu -or ($View.state.filter_picker.category|ConvertTo-Json -Compress) -ne $filterCategory -or $View.state.filter_picker.selected -ne $filterSelected){throw 'Filter locale publication changed category, selected filter, document history or GPU ownership'}
}
function Filter-Surfaces{
 Use-Window $first;Property-Panel 'adjustments'
 if($null -ne (Fresh-Model).state.filter_picker.search){Invoke-Id 'filter-search-toggle';Wait-Until {$view=Model;$view -and $null -eq $view.state.filter_picker.search} 'Filter search did not close before the retained inventory proof' 15}
 $baseline=Fresh-Model
 if($null -ne $baseline.state.filter_picker.category -or !@($baseline.state.adjustments).Count){throw 'Filter retention requires the real All category and populated shared results'}
 $filterDocument=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$filterGpu=$baseline.windows_gpu_generation
 $filterCategory=$baseline.state.filter_picker.category|ConvertTo-Json -Compress;$filterSelected=$baseline.state.filter_picker.selected
 $filterInventory=@($baseline.state.adjustments|ForEach-Object {Select-Object -InputObject $_ -Property id,category,icon,category_icon,animated,action})|ConvertTo-Json -Depth 12 -Compress
 $ids=@('filter-picker','filter-category','filter-search-toggle','filter-list')+@($baseline.state.adjustments|ForEach-Object {'filter-'+$_.id})
 $identities=Native-Identities $ids;$optionIds=@(Filter-Options|ForEach-Object {$_.GetRuntimeId() -join ':'})
 $selectedIdentity=(Selected-Option (Control 'filter-category')).GetRuntimeId() -join ':'
 Surface-Languages 'retained-filter-results' {
  $view=Fresh-Model $first $choice.tag;Filter-Owner $view;Check-Identities $identities
  foreach($id in @('filter-picker','filter-list')){if((Control $id).Current.Name -ne (Catalog-Text $choice.tag 'workspace-panel-adjustments' 'workspace')){throw "Retained filter container accessibility title stayed stale: $id"}}
  $inventory=@($view.state.adjustments|ForEach-Object {Select-Object -InputObject $_ -Property id,category,icon,category_icon,animated,action})|ConvertTo-Json -Depth 12 -Compress
  if($null -ne $view.state.filter_picker.search -or $inventory -ne $filterInventory -or ((Selected-Option (Control 'filter-category')).GetRuntimeId() -join ':') -ne $selectedIdentity){throw 'Filter result inventory or selected category native identity changed during locale publication'}
  $options=@(Filter-Options)
  foreach($i in 0..($options.Count-1)){if(($options[$i].GetRuntimeId() -join ':') -ne $optionIds[$i] -or $options[$i].Current.Name -ne $view.state.filter_categories[$i].label){throw 'Retained filter category option identity or caption stayed stale'}}
  if((Control 'filter-category').Current.Name -ne (Catalog-Text $choice.tag 'native-color-category' 'common') -or (Control 'filter-search-toggle').Current.Name -ne (Catalog-Text $choice.tag 'resources-search-filters' 'resources')){throw 'Filter category or search toggle retained an earlier language'}
  foreach($result in $view.state.adjustments){$row=Control ('filter-'+$result.id);$status=if($result.id -eq $filterSelected){Catalog-Text $choice.tag 'native-search-selected' 'common'}else{''};if($row.Current.Name -ne $result.label -or $row.Current.ItemStatus -ne $status){throw 'Filter result caption or selected accessibility status stayed stale'}}
  Check-VisibleText 'filter-category' $view.state.filter_categories[0].label
 }
 Invoke-Id 'filter-search-toggle';Wait-Until {$view=Model;$view -and $null -ne $view.state.filter_picker.search -and (Find 'filter-search' -Visible)} 'Filter search did not open its real native editor' 15
 $search=Control 'filter-search' -Arranged;Select-Draft $search $literalName
 Wait-Until {$view=Model;$view -and $view.state.filter_picker.search -eq $literalName -and @($view.state.adjustments).Count -eq 0 -and (Find 'filter-empty' -Visible)} 'Literal Unicode filter query did not acknowledge its empty result state' 15
 $identities=Native-Identities @('filter-picker','filter-search','filter-search-toggle','filter-list','filter-empty')
 Surface-Languages 'retained-filter-query' {
  $view=Fresh-Model $first $choice.tag;Filter-Owner $view;Check-Identities $identities;$entry=Control 'filter-search'
  foreach($id in @('filter-picker','filter-list')){if((Control $id).Current.Name -ne (Catalog-Text $choice.tag 'workspace-panel-adjustments' 'workspace')){throw "Retained filter query container accessibility title stayed stale: $id"}}
  if($view.state.filter_picker.search -ne $literalName -or @($view.state.adjustments).Count -ne 0 -or (Value $entry) -ne $literalName -or (Selection $entry) -ne $literalName -or !$entry.Current.HasKeyboardFocus){throw 'Filter search lost its literal Unicode query, native selection, restored keyboard focus or empty result state'}
  $caption=Catalog-Text $choice.tag 'resources-search-filters' 'resources'
  if($entry.Current.Name -ne $caption -or (Control 'filter-search-toggle').Current.Name -ne $caption){throw 'Retained filter query editor or toggle caption stayed stale'}
  Check-VisibleText 'filter-empty' (Catalog-Text $choice.tag 'resources-no-matching-filters' 'resources')
 }
 Invoke-Id 'filter-search-toggle';Wait-Until {$view=Model;$view -and $null -eq $view.state.filter_picker.search -and @($view.state.adjustments).Count -gt 0} 'Filter results did not return after cancelling literal search' 15
}
function Property-Filter([string]$Id){
 Property-Panel 'adjustments'
 if($null -ne (Fresh-Model).state.filter_picker.search){
  Invoke-Id 'filter-search-toggle'
  Wait-Until {$view=Model;$view -and $null -eq $view.state.filter_picker.search} 'Native filter search did not close before reading the complete inventory' 20
 }
 $view=Fresh-Model
 if($null -ne $view.state.filter_picker.category){throw 'Private Properties fixture requires the unchanged All filters category'}
 $filter=@($view.state.adjustments|Where-Object id -eq $Id)[0]
 if(!$filter){throw "Shared filter inventory is missing $Id"}
 Invoke-Id 'filter-search-toggle'
 (Control 'filter-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($filter.label)
 Wait-Until {$view=Model;$view -and $view.state.filter_picker.search -eq $filter.label -and (Find ('filter-'+$Id))} "Native filter search did not resolve $Id" 20
 Invoke-Id ('filter-'+$Id)
 Wait-Until {$view=Model;$view -and $view.state.layer_properties.description -eq $filter.label} "Inserted filter did not become active: $Id" 20
 Property-Panel 'properties'
}
function Property-Values($View){
 $View.state.layer_properties|Select-Object layer,page,@{Name='values';Expression={@($_.controls|Select-Object key,value)}}|ConvertTo-Json -Depth 40 -Compress
}
function Property-Owner($View){
 if(($View.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $propertyDocument -or $View.windows_gpu_generation -ne $propertyGpu -or (Property-Values $View) -ne $propertyValues){
  @{expected_document=$propertyDocument;actual_document=($View.state.document_file|ConvertTo-Json -Depth 20 -Compress);expected_gpu=$propertyGpu;actual_gpu=$View.windows_gpu_generation;expected_properties=$propertyValues;actual_properties=(Property-Values $View)}|ConvertTo-Json -Depth 40|Set-Content (Join-Path $run 'retained-properties-owner-failure.json')
  throw 'Locale publication changed retained effect values, page, document history or GPU ownership'
 }
}
function Property-Surfaces{
 Use-Window $first
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $first.hwnd -Action Resize -Width 1550 -Height 1400
 Property-Filter 'color_balance'
 $view=Fresh-Model
 $midtones=@($view.state.layer_properties.pages|Where-Object id -eq 'midtones')[0]
 Choose-Option 'properties-page' $midtones.label
 Wait-Until {$view=Model;$view -and $view.state.layer_properties.page -eq 'midtones' -and (Find 'property-midtones_red')} 'Properties page did not select its semantic midtones page' 20
 $entry=Control 'property-midtones_red';Select-Draft $entry $numericDraft
 $baseline=Fresh-Model;$propertyDocument=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$propertyGpu=$baseline.windows_gpu_generation;$propertyValues=Property-Values $baseline
 $identities=Native-Identities @('layer-properties','properties-page','property-midtones_red','property-preserve_luminance')
 $pageIdentity=(Selected-Option (Control 'properties-page')).GetRuntimeId() -join ':'
 Surface-Languages 'retained-properties-page-numeric-toggle' {
  $view=Fresh-Model $first $choice.tag;Property-Owner $view;Check-Identities $identities
  $page=@($view.state.layer_properties.pages|Where-Object id -eq 'midtones')[0]
  $number=@($view.state.layer_properties.controls|Where-Object key -eq 'midtones_red')[0];$toggle=@($view.state.layer_properties.controls|Where-Object key -eq 'preserve_luminance')[0]
  if((Selected-Option (Control 'properties-page')).Current.Name -ne $page.label -or ((Selected-Option (Control 'properties-page')).GetRuntimeId() -join ':') -ne $pageIdentity){throw 'Properties page option lost its identity or current caption'}
  Check-VisibleText 'properties-page' $page.label
  $current=Control 'property-midtones_red'
  if((Value $current) -ne $numericDraft -or (Selection $current) -ne $numericDraft -or $current.Current.Name -ne (Catalog-Text $choice.tag 'numeric-edit-label' 'common').Replace('{ $label }',$number.label)){throw 'Properties numeric editor lost its Unicode draft, selected literal or current caption'}
  $expectedToggle=if($toggle.value.value){[System.Windows.Automation.ToggleState]::On}else{[System.Windows.Automation.ToggleState]::Off}
  if((Control 'property-preserve_luminance').Current.Name -ne $toggle.label -or (Control 'property-preserve_luminance').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne $expectedToggle){throw 'Properties toggle lost its raw state or current caption'}
 }
 (Control 'property-midtones_red').SetFocus();Key 27
 Property-Filter 'curves'
 $graph=Control 'property-rgb-curve' -Arranged;$bounds=$graph.Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',[int]($bounds.Left+$bounds.Width*.5),[int]($bounds.Top+$bounds.Width*.25));[CapyRowPointer]::Up()
 (Fresh-Model)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run 'curve-contact-before-settlement-model.json')
 Wait-Until {$view=Model;$view -and @($view.state.layer_properties.controls|Where-Object key -eq 'rgb')[0].curve.selected -eq 1 -and @($view.state.layer_properties.controls|Where-Object key -eq 'rgb')[0].value.value.Count -eq 3 -and @($view.state.commands|Where-Object id -eq 'undo')[0].enabled} 'Native curve release did not commit and select its interior precise point' 20
 Select-Draft (Control 'property-rgb-output') $numericDraft
 $baseline=Fresh-Model;$propertyDocument=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$propertyGpu=$baseline.windows_gpu_generation;$propertyValues=Property-Values $baseline
 $baseline|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run 'curve-before-locale-model.json')
 $choiceValue=$baseline.state.layer_properties.page
 $choicePage=@($baseline.state.layer_properties.pages|Where-Object id -eq $choiceValue)[0]
 if(!$choicePage -or $choiceValue -ne 'rgb'){throw 'Curves fixture did not expose its actual RGB channel page choice'}
 $choiceId='properties-page'
 $identities=Native-Identities @('layer-properties',$choiceId,'property-rgb-curve','property-rgb-input','property-rgb-output')
 $curveBefore=@($baseline.state.layer_properties.controls|Where-Object key -eq 'rgb')[0].curve
 $optionIdentity=(Selected-Option (Control $choiceId)).GetRuntimeId() -join ':'
 Surface-Languages 'retained-properties-choice-curve-coordinate' {
  $view=Fresh-Model $first $choice.tag;Property-Owner $view;Check-Identities $identities
  $page=@($view.state.layer_properties.pages|Where-Object id -eq $choiceValue)[0];$expected=$page.label
  if(!$page -or $view.state.layer_properties.page -ne $choiceValue -or (Control $choiceId).Current.Name -ne $view.state.layer_properties.title -or (Selected-Option (Control $choiceId)).Current.Name -ne $expected -or ((Selected-Option (Control $choiceId)).GetRuntimeId() -join ':') -ne $optionIdentity){throw 'Properties choice lost its current label or retained semantic option'}
  Scroll-Position (Control $choiceId) 0
  Check-VisibleText $choiceId $expected
  $curve=@($view.state.layer_properties.controls|Where-Object key -eq 'rgb')[0].curve;$output=Control 'property-rgb-output'
  if($curve.epoch -ne $curveBefore.epoch -or $curve.selected -ne $curveBefore.selected -or $curve.input.value -ne $curveBefore.input.value -or $curve.output.value -ne $curveBefore.output.value -or (Value $output) -ne $numericDraft -or (Selection $output) -ne $numericDraft){throw 'Precise curve coordinate lost its raw point, owner epoch, pending Unicode draft or selected literal'}
  if($output.Current.Name -ne (Catalog-Text $choice.tag 'numeric-edit-label' 'common').Replace('{ $label }',$curve.axes[1].label) -or (Control 'property-rgb-curve').Current.HelpText -ne $curve.help){throw 'Precise curve field or graph help did not follow current shared captions'}
 }
 (Control 'property-rgb-output').SetFocus();Key 27
 foreach($surface in @(@{filter='split_tone';key='shadows'},@{filter='gradient_map';key='gradient'})){
  Property-Filter $surface.filter;$colorId='property-'+$surface.key+'-color'
  Invoke-Id $colorId
  Wait-Until {(Find 'edit-color-0-0')} 'Edit Color did not open' 20
  Invoke-Id 'edit-color-0-0';$colorEntry=Control 'edit-color-0-0-entry';Select-Draft $colorEntry $numericDraft;Key 13
  $baseline=Fresh-Model;$propertyDocument=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$propertyGpu=$baseline.windows_gpu_generation;$propertyValues=Property-Values $baseline
  $colorIds=@($colorId,'edit-color-form-0','edit-color-apply','edit-color-error','edit-color-0-0-entry','edit-color-0-1','edit-color-0-2')
  if($surface.key -eq 'gradient'){$colorIds+=@('property-gradient-gradient','property-gradient-position','property-gradient-interpolation','property-gradient-reverse','property-gradient-remove','property-gradient-reset','property-gradient-use-color')}
  $identities=Native-Identities $colorIds
  $otherValues=@(1..2|ForEach-Object {Shown-Value (Control ('edit-color-0-'+$_))})
  Surface-Languages ('retained-color-form-'+$surface.filter) {
   $view=Fresh-Model $first $choice.tag;Property-Owner $view;Check-Identities $identities
   $current=Control 'edit-color-0-0-entry'
   if((Value $current) -ne $numericDraft){throw 'Edit Color lost its native field or Unicode draft'}
   foreach($i in 1..2){if((Shown-Value (Control ('edit-color-0-'+$i))) -ne $otherValues[$i-1]){throw 'Edit Color changed another retained value'}}
   if($current.Current.Name -ne (Catalog-Text $choice.tag 'settings-red' 'settings')){throw 'The open color value retained an earlier language caption'}
   foreach($i in 1..2){$field=@('red','green','blue')[$i];if(!(Control ('edit-color-0-'+$i)).Current.Name.StartsWith((Catalog-Text $choice.tag ('settings-'+$field) 'settings'))){throw 'Color value retained an earlier language caption'}}
   if((Control 'edit-color-form-0').Current.Name -ne 'RGB' -or (Control 'edit-color-apply').Current.IsEnabled){throw 'Edit Color lost its format or enabled a refused draft'}
   if((Control 'edit-color-error').Current.Name -ne (Catalog-Text $choice.tag 'color-form-finite-field').Replace('{ $label }',(Catalog-Text $choice.tag 'settings-red' 'settings'))){throw 'Edit Color refusal did not follow the current language'}
  }
  Invoke-Id 'edit-color-cancel';Wait-Until {!(Find 'edit-color-apply')} 'Cancel did not close Edit Color'
 }
 Menu-Command 'file' 'new_document'
 (Control 'document-width').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 (Control 'document-height').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 $creation=(Fresh-Model).document_options.creation;$floatDepth=@($creation.depths|Where-Object {$_[0] -eq 'F16'})[0]
 if(!$floatDepth){throw 'Shared creation inventory has no F16 semantic depth'}
 Choose-Option 'document-depth' $floatDepth[1];Invoke 'PrimaryButton'
 Wait-Until {$view=Model;$view -and $view.color_panel.hdr -and $view.color_panel.document_depth -eq 'F16' -and $view.state.tabs[-1].width -eq 64} 'Floating test document did not prepare its HDR color controls' 45
 Property-Filter 'gradient_map';$colorId='property-gradient-color';Invoke-Id $colorId
 Wait-Until {(Find 'edit-color-intensity')} 'Floating document Edit Color did not expose intensity' 20
 Invoke-Id 'edit-color-intensity';Select-Draft (Control 'edit-color-intensity-entry') '12+';Key 13
 $baseline=Fresh-Model;$propertyDocument=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$propertyGpu=$baseline.windows_gpu_generation;$propertyValues=Property-Values $baseline
 $identities=Native-Identities @($colorId,'edit-color-form-0','edit-color-intensity-entry','edit-color-apply','edit-color-error')
 Surface-Languages 'retained-hdr-color-intensity-error' {
  $view=Fresh-Model $first $choice.tag;Property-Owner $view;Check-Identities $identities
  $intensity=Control 'edit-color-intensity-entry'
  if(!$view.color_panel.hdr -or $view.color_panel.document_depth -ne 'F16' -or (Value $intensity) -ne '12+' -or (Selection $intensity) -ne '12+' -or (Control 'edit-color-apply').Current.IsEnabled){throw 'Floating Edit Color lost its invalid EV draft, selection or refusal eligibility'}
  if($intensity.Current.Name -ne (Catalog-Text $choice.tag 'native-color-intensity-ev' 'common') -or (Control 'edit-color-error').Current.Name -ne (Catalog-Text $choice.tag 'color-form-finite-field').Replace('{ $label }',(Catalog-Text $choice.tag 'native-color-intensity-ev' 'common'))){throw 'Floating Edit Color EV syntax refusal did not use current canonical copy'}
 }
 Select-Draft (Control 'edit-color-intensity-entry') '1.25';Key 13
 Wait-Until {!(Find 'edit-color-intensity-entry')} 'Floating Edit Color did not accept the corrected EV'
 $identities=Native-Identities @($colorId,'edit-color-form-0','edit-color-intensity','edit-color-apply','edit-color-error')
 Surface-Languages 'retained-hdr-color-intensity-retry' {
  $view=Fresh-Model $first $choice.tag;Property-Owner $view;Check-Identities $identities
  $intensity=Control 'edit-color-intensity'
  if(!(Control 'edit-color-apply').Current.IsEnabled -or !$intensity.Current.Name.EndsWith('+1.25 EV')){throw 'Floating Edit Color retry did not accept the corrected EV'}
  if(!$intensity.Current.Name.StartsWith((Catalog-Text $choice.tag 'native-color-intensity-ev' 'common')) -or (Control 'edit-color-error').Current.Name){throw 'Floating Edit Color retry kept a stale refusal or caption'}
 }
 Invoke-Id 'edit-color-cancel';Wait-Until {!(Find 'edit-color-apply')} 'Cancel did not close Edit Color'
 Use-Window $textWindow
}

function Tool-Surfaces{
 Use-Window $first
 Search-Command 'transform' 'scale_rotate'
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.tool -eq 'transform' -and (Find 'tool-setting-transform_x') -and (Find 'tool-choice-transform-reference')} 'Transform did not expose retained native position and anchor controls' 30
 $entry=Control 'tool-setting-transform_x' -Arranged;Select-Draft $entry $numericDraft
 $baseline=Fresh-Model;$document=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=$baseline.windows_gpu_generation
 $settings=@($baseline.state.tool_settings|ForEach-Object {@{id=$_.id;value=$_.value}})|ConvertTo-Json -Depth 8 -Compress
 $extras=@($baseline.state.tool_extra|ForEach-Object {@{id=$_.Choice.id;items=@($_.Choice.items|ForEach-Object {@{action=$_.action;selected=$_.selected}})}})|ConvertTo-Json -Depth 12 -Compress
 $ids=@('tool-setting-transform_x','tool-setting-transform_y','tool-choice-transform-reference','tool-action-cancel_transform')
 foreach($i in 0..8){$ids+=('tool-choice-transform-reference-'+$i)}
 foreach($kind in @('groups','subtools')){for($i=0;$i -lt @($baseline.state.tool_set.$kind).Count;$i++){$ids+=($(if($kind -eq 'groups'){'tool-group-'}else{'tool-subtool-'})+$i)}}
 $identities=Native-Identities $ids
 Surface-Languages 'retained-transform-position-anchor' {
  $view=Fresh-Model $first $choice.tag;Check-Identities $identities
  $currentSettings=@($view.state.tool_settings|ForEach-Object {@{id=$_.id;value=$_.value}})|ConvertTo-Json -Depth 8 -Compress
  $currentExtras=@($view.state.tool_extra|ForEach-Object {@{id=$_.Choice.id;items=@($_.Choice.items|ForEach-Object {@{action=$_.action;selected=$_.selected}})}})|ConvertTo-Json -Depth 12 -Compress
  if(($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $view.windows_gpu_generation -ne $gpu -or $currentSettings -ne $settings -or $currentExtras -ne $extras){throw 'Locale publication changed raw transform coordinates, reference selection, document history or GPU owner'}
  $field=@($view.state.tool_settings|Where-Object id -eq 'transform_x')[0];$current=Control 'tool-setting-transform_x'
  if((Value $current) -ne $numericDraft -or (Selection $current) -ne $numericDraft -or $current.Current.Name -ne (Catalog-Text $choice.tag 'numeric-edit-label' 'common').Replace('{ $label }',$field.label)){throw 'Transform numeric editor lost its selected literal draft, identity or current caption'}
  $anchor=@($view.state.tool_extra|Where-Object {$_.Choice.id -eq 'transform-reference'})[0].Choice
  if((Control 'tool-choice-transform-reference').Current.Name -ne $anchor.label){throw 'Retained transform anchor grid caption stayed in an earlier language'}
  foreach($i in 0..8){$cell=Control ('tool-choice-transform-reference-'+$i);$item=$anchor.items[$i]
   $status=if($item.selected){(Catalog-Text $choice.tag 'native-search-selected' 'common')}else{''}
   if($cell.Current.Name -ne $item.label -or $cell.Current.ItemStatus -ne $status){throw 'Retained transform anchor cell lost its current caption or raw selection status'}
  }
  foreach($kind in @('groups','subtools')){for($i=0;$i -lt @($view.state.tool_set.$kind).Count;$i++){
   $item=$view.state.tool_set.$kind[$i];$control=Control ($(if($kind -eq 'groups'){'tool-group-'}else{'tool-subtool-'})+$i)
   $status=if($item.selected){(Catalog-Text $choice.tag 'native-search-selected' 'common')}else{''}
   if($control.Current.Name -ne $item.label -or $control.Current.ItemStatus -ne $status){throw 'Retained tool-set button caption or selection status stayed in an earlier language'}
  }}
  $cancel=@($view.state.commands|Where-Object id -eq 'cancel_transform')[0]
  if((Control 'tool-action-cancel_transform').Current.Name -ne $cancel.label){throw 'Retained transform action caption stayed in an earlier language'}
 }
 (Control 'tool-setting-transform_x').SetFocus();Key 27
 Invoke-Id 'tool-action-cancel_transform'
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.tool -ne 'transform' -and @($view.state.tool_actions|Where-Object command -eq 'cancel_transform').Count -eq 0} 'Transform cancellation did not leave the retained draft' 30
 if(((Fresh-Model).state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document){throw 'Cancelling the uncommitted transform draft changed document history'}
 Search-Command 'Tonal range' 'tonal_select'
 Wait-Until {$view=Model;$view -and @($view.state.tool_extra|Where-Object {$_.Choice.id -eq 'tonal-tones'}).Count -eq 1 -and (Find 'tool-choice-tonal-tones-0')} 'Tonal action did not publish its current native choices' 30
 $tones=@((Fresh-Model).state.tool_extra|Where-Object {$_.Choice.id -eq 'tonal-tones'})[0].Choice;$custom=-1
 for($i=0;$i -lt $tones.items.Count;$i++){if($tones.items[$i].icon -eq 'tonal-custom'){$custom=$i;break}}
 if($custom -lt 0){throw 'Shared tonal choices did not offer their semantic Custom item'}
 Invoke-Id ('tool-choice-tonal-tones-'+$custom)
 Wait-Until {(Find 'tool-setting-tonal_lower') -and (Find 'tool-setting-tonal_upper') -and (Find 'tool-setting-range')} 'Tonal tool did not expose its retained native range' 30
 $lower=Control 'tool-setting-tonal_lower' -Arranged;Select-Draft $lower $numericDraft
 $baseline=Fresh-Model;$document=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=$baseline.windows_gpu_generation
 $bounds=@($baseline.state.tool_settings|Where-Object {$_.id -in @('tonal_lower','tonal_upper')}|ForEach-Object {@{id=$_.id;value=$_.value}})|ConvertTo-Json -Depth 8 -Compress
 $upperText=Value (Control 'tool-setting-tonal_upper');$identities=Native-Identities @('tool-setting-tonal_lower','tool-setting-tonal_upper','tool-setting-range','tool-setting-range-track')
 Surface-Languages 'retained-tonal-range' {
  $view=Fresh-Model $first $choice.tag;Check-Identities $identities
  $currentBounds=@($view.state.tool_settings|Where-Object {$_.id -in @('tonal_lower','tonal_upper')}|ForEach-Object {@{id=$_.id;value=$_.value}})|ConvertTo-Json -Depth 8 -Compress
  if(($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $view.windows_gpu_generation -ne $gpu -or $currentBounds -ne $bounds -or (Value (Control 'tool-setting-tonal_lower')) -ne $numericDraft -or (Selection (Control 'tool-setting-tonal_lower')) -ne $numericDraft -or (Value (Control 'tool-setting-tonal_upper')) -ne $upperText){throw 'Locale publication changed the retained tonal range values, pending draft/selection or document/GPU owner'}
  $hint=Catalog-Text $choice.tag 'toolbar-range-in-stops-relative-to-reference-white-0' 'toolbar'
  foreach($id in @('tool-setting-range','tool-setting-range-track')){if((Control $id).Current.Name -ne $hint){throw 'Retained tonal range caption stayed in an earlier language'}}
  foreach($id in @('tonal_lower','tonal_upper')){$field=@($view.state.tool_settings|Where-Object id -eq $id)[0];$expected=(Catalog-Text $choice.tag 'numeric-edit-label' 'common').Replace('{ $label }',($field.label+' — '+$hint));if((Control ('tool-setting-'+$id)).Current.Name -ne $expected){throw 'Retained tonal numeric accessibility caption stayed in an earlier language'}}
 }
 (Control 'tool-setting-tonal_lower').SetFocus();Key 27
 $priorTool=(Fresh-Model).state.layer_tools.tool|ConvertTo-Json -Depth 8 -Compress
 Search-Command 'Eyedropper' 'eyedropper'
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.tool -in @('pick_visible','pick_layer') -and (Find 'picker-setting-source') -and (Find 'picker-setting-size')} 'Sampler did not expose its native choices' 30
 $baseline=Fresh-Model;$document=$baseline.state.document_file|ConvertTo-Json -Depth 20 -Compress;$gpu=$baseline.windows_gpu_generation;$picker=$baseline.state.color_picker
 foreach($choice in $seen){
  Use-Window $second
  Wait-Until {$view=Model $first;$view -and ($view.state.layer_tools.tool|ConvertTo-Json -Depth 8 -Compress) -eq $priorTool} 'Normal Preferences activation did not restore the prior tool before locale publication' 30
  $blur=Fresh-Model $first
  if(($blur.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $blur.windows_gpu_generation -ne $gpu -or $blur.state.color_picker.layer -ne $picker.layer -or $blur.state.color_picker.sample_width -ne $picker.sample_width){throw 'Expected picker Blur changed raw choices or document/GPU ownership'}
  Language-Choice $choice.index|Out-Null
  Wait-Until {(Model $first).windows_active_tag -eq $choice.tag} 'Sampler window did not adopt the current language' 30
  Use-Window $first
  $published=Fresh-Model $first $choice.tag
  if(($published.state.layer_tools.tool|ConvertTo-Json -Depth 8 -Compress) -ne $priorTool){throw 'Locale publication changed the explicitly restored prior tool'}
  Search-Command 'Eyedropper' 'eyedropper'
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $choice.tag -and $view.state.layer_tools.tool -in @('pick_visible','pick_layer') -and (Find 'picker-setting-source') -and (Find 'picker-setting-size')} 'Sampler did not reenter through its native command' 30
  $view=Fresh-Model $first $choice.tag;$current=$view.state.color_picker
  if(($view.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $document -or $view.windows_gpu_generation -ne $gpu -or $current.layer -ne $picker.layer -or $current.sample_width -ne $picker.sample_width){throw 'Picker reentry after locale publication changed raw choices or document/GPU ownership'}
  $source=Control 'picker-setting-source';$size=Control 'picker-setting-size'
  $sourceText=Catalog-Text $choice.tag $(if($current.layer){'toolbar-selected-layer'}else{'toolbar-visible-color'}) 'toolbar'
  $sizeId=switch([int]$current.sample_width){1{'toolbar-single-pixel'}5{'toolbar-5-px-circle'}15{'toolbar-15-px-circle'}51{'toolbar-51-px-circle'}101{'toolbar-101-px-circle'}default{throw 'Unexpected shared sampler size'}}
  $sizeText=Catalog-Text $choice.tag $sizeId 'toolbar'
  if($source.Current.Name -ne (Catalog-Text $choice.tag 'toolbar-source' 'toolbar') -or $size.Current.Name -ne (Catalog-Text $choice.tag 'toolbar-sample-size' 'toolbar') -or (Selected-Option $source).Current.Name -ne $sourceText -or (Selected-Option $size).Current.Name -ne $sizeText){throw 'Native sampler option did not use its current canonical caption'}
  Check-VisibleText 'picker-setting-source' $sourceText;Check-VisibleText 'picker-setting-size' $sizeText
  Capture-Window $first ($choice.tag+'-sampler-after-switch')
 }
 Search-Command 'Pen' 'pen'
}

function Notice-Surface{
 Use-Window $second
 if(!(Model).preferences){Menu-Command 'edit' 'settings'}
 Invoke-Id 'preference-page-appearance'
 $japanese=1+[Array]::IndexOf($shippedTags,'ja')
 if($japanese -lt 1){throw 'The registered Japanese locale is missing'}
 Language-Choice $japanese|Out-Null;Language-Choice 1|Out-Null
 Use-Window $first
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 Menu-Command 'file' 'open_document'
 Choose-Path (Join-Path $repo 'apps/layer-web/fixtures/shared-image-f64-builtin.capy')
 Wait-Until {@((Model).state.layers|Where-Object object).Count -eq 3 -and (Model).brush_ready} 'The notice image fixture did not prepare' 90
 $layer=@((Model).state.layers|Where-Object object)[0].id
 Invoke-Id "layer-$layer-name"
 Focus-Canvas;Key 66
 Wait-Until {((Model).state.commands|Where-Object id -eq 'brush').selected} 'The brush did not activate for the image refusal'
 $caseIndex=0;$wide=@{height=$null;copy=$null}
 foreach($locked in @($false,$true,$false)){
  $caseIndex++;$caseName=('{0:D2}-{1}' -f $caseIndex,$(if($locked){'narrow-locked'}else{'wide-unlocked'}))
  Use-Window $second;Language-Choice 1|Out-Null
  Wait-Until {(Model $first).windows_active_tag -eq 'en'} 'The image window did not return to English'
  Use-Window $first
  if($locked){
   $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
   Wait-Until {$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Normal} 'The notice window did not restore before resize'
   Resize-Window $first 1400 950
  }else{
   $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
   Wait-Until {$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Current.WindowVisualState -eq [System.Windows.Automation.WindowVisualState]::Maximized} 'The wide notice window did not maximize'
  }
  Focus-Canvas
  $null=Control 'drawing-canvas' -Arranged
  $geometry=@{bounds=$null;camera=$null;work=$null;view=$null}
  Wait-Until {
   $view=Model;$canvas=Find 'drawing-canvas' -Visible
   if(!$view -or !$canvas -or !$view.state.camera -or !$view.layout.work_area){return $false}
   $bounds=$canvas.Current.BoundingRectangle;$camera=$view.state.camera;$layout=$view.layout;$work=$layout.work_area
   if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0 -or $work.width -le 0 -or $work.height -le 0){return $false}
   $density=[CapyRowPointer]::GetDpiForWindow([IntPtr]$first.hwnd)/96.
   $size=@($bounds.Width,$bounds.Height);$area=@($work.x,$work.y,$work.width,$work.height)
   foreach($axis in 0,1){if([Math]::Abs($camera.viewport[$axis]-$size[$axis]) -gt 1 -or [Math]::Abs($layout.viewport[$axis]*$density-$size[$axis]) -gt 1){return $false}}
   foreach($axis in 0,1,2,3){if([Math]::Abs($camera.work_area[$axis]-$area[$axis]*$density) -gt 1){return $false}}
   $geometry.bounds=$bounds;$geometry.camera=$camera;$geometry.work=$work;$geometry.view=$view;$true
  } 'The notice camera and work area did not match the current native canvas' 15
  if($locked -and $geometry.work.width -gt 320){throw "The notice work area is not narrow: $($geometry.work.width) DIPs"}
  $bounds=$geometry.bounds;$camera=$geometry.camera
  $header=Control 'title-bar';$physical=@{}
  foreach($part in (@{window=$root.Current.BoundingRectangle;canvas=$bounds;header=$header.Current.BoundingRectangle}).GetEnumerator()){
   $box=$part.Value;$physical[$part.Key]=@{x=$box.X;y=$box.Y;width=$box.Width;height=$box.Height}
  }
  $dpi=[CapyRowPointer]::GetDpiForWindow([IntPtr]$first.hwnd);$view=$geometry.view
  @{case=$caseName;theme=$Theme;utc=[DateTime]::UtcNow.ToString('o');process_id=$review.Id;window_id=$first.id;hwnd=$first.hwnd;dpi=$dpi;density=$dpi/96.;physical_bounds=$physical;
   layout=@{viewport=$view.layout.viewport;work_area=$view.layout.work_area;status=$view.layout.status};camera=$camera;
   header=@{presentation=($header.Current.ItemStatus|ConvertFrom-Json);model=$view.header.model;presentation_height=$view.header_presentation_height;titlebar_insets=$view.titlebar_insets};
   pre_contact_layer_locked=$view.state.layer_tools.editing_layer.locked;expected_locked_after_publication=$locked}|ConvertTo-Json -Depth 30|Set-Content (Join-Path $run ("notice-geometry-$caseName.json"))
  $at=@([int]($bounds.X+$camera.work_area[0]+$camera.work_area[2]*.5),[int]($bounds.Y+$camera.work_area[1]+$camera.work_area[3]*.5))
  $notice=@{before=$null;updated=$null;bounds=$null;stable=0;height=$null}
  [CapyRowPointer]::Down('pen',$at[0],$at[1]);[CapyRowPointer]::Up()
  Wait-Until {
   $view=Model
   if(@($view.state.notice.actions).Count -ne 3 -or !(Find 'canvas-notice-action-add_mask' -Visible)){return $false}
   $notice.before=$view.state.notice;$true
  } 'The image refusal did not publish three visible actions'
  $before=$notice.before;$clock=[Diagnostics.Stopwatch]::StartNew();$timing=@{case=$caseName;published_utc=[DateTime]::UtcNow.ToString('o')}
  $identities=@{};foreach($offer in $before.actions){$identities[$offer.id]=(Control ('canvas-notice-action-'+$offer.id)).GetRuntimeId() -join ':'}
  if($locked){(Control 'layer-lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();Wait-Until {(Model).state.layer_tools.editing_layer.locked} 'The image layer did not lock'}
  Use-Window $second
  while($clock.Elapsed.TotalMilliseconds -lt 1500){Start-Sleep -Milliseconds 25}
  Language-Choice $japanese|Out-Null
  Wait-Until {
   $view=Model $first
   if($view.windows_active_tag -ne 'ja' -or !$view.state.notice -or $view.state.notice.id -ne $before.id -or $view.state.notice.text -eq $before.text){return $false}
   $notice.updated=$view.state.notice;$true
  } 'Language publication did not retain and relabel the shared notice' 3
  Use-Window $first
  $updated=$notice.updated
  if((@($updated.actions.id) -join ',') -ne 'add_mask,new_paint_layer,rasterize_layer'){throw 'The image notice action order changed'}
  foreach($offer in $updated.actions){
   $enabled=!$locked -or $offer.id -eq 'new_paint_layer'
   if($offer.enabled -ne $enabled -or (!$enabled -and !$offer.reason)){throw 'The image notice did not publish the expected locked action availability and reasons'}
  }
  Wait-Until {
   $frame=Find 'canvas-notice' -Visible;$text=if($frame){Find 'canvas-notice-text' -Within $frame -Visible}
   if(!$frame -or !$text -or $text.Current.Name -ne $updated.text -or $frame.Current.Name -ne $updated.text){return $false}
   $bounds=$frame.Current.BoundingRectangle
   $textBounds=$text.Current.BoundingRectangle;$actionBounds=@()
   if(!$bounds.Contains($textBounds)){return $false}
   foreach($offer in $updated.actions){
    $control=Find ('canvas-notice-action-'+$offer.id) -Within $frame -Visible
    if(!$control){return $false}
    $controlBounds=$control.Current.BoundingRectangle
    if(!$bounds.Contains($controlBounds)){return $false}
    if($locked){if($controlBounds.Top+1 -lt $textBounds.Bottom){return $false}}
    elseif($controlBounds.Left+1 -lt $textBounds.Right){return $false}
    $actionBounds+=@{id=$offer.id;bounds=$controlBounds}
    if(($control.GetRuntimeId() -join ':') -ne $identities[$offer.id]){throw 'Relabeling replaced a retained notice action'}
    if($control.Current.Name -ne $offer.label -or $control.Current.IsEnabled -ne $offer.enabled -or $control.Current.HelpText -ne [string]$offer.reason){return $false}
    $caption=Find $offer.label -Name -Within $control -Type ([System.Windows.Automation.ControlType]::Text) -Visible
    if(!$caption){return $false}
    $captionBounds=$caption.Current.BoundingRectangle
    if($caption.Current.Name -ne $offer.label -or $captionBounds.IsEmpty -or $captionBounds.Width -le 0 -or $captionBounds.Height -le 0 -or !$controlBounds.Contains($captionBounds) -or !$bounds.Contains($captionBounds)){return $false}
   }
   if($notice.bounds -eq $bounds){$notice.stable++}else{$notice.bounds=$bounds;$notice.stable=0}
   $notice.layout=@{frame=$bounds;text=$textBounds;actions=$actionBounds}
   $notice.height=$bounds.Height;$notice.stable -ge 1
  } 'The retained visible native notice did not adopt its shared text and actions before expiry' 2
  $timing.native_verified_ms=$clock.Elapsed.TotalMilliseconds
  if(!$locked){
   $density=[CapyRowPointer]::GetDpiForWindow([IntPtr]$first.hwnd)/96.
   $copy=$updated|Select-Object text,actions|ConvertTo-Json -Depth 6 -Compress
   if($null -eq $wide.height){$wide.height=$notice.height/$density;$wide.copy=$copy}
   elseif($copy -ne $wide.copy -or [Math]::Abs($notice.height/$density-$wide.height) -gt 1){throw "The matching wide notice did not return to its original height: expected $($wide.height) DIPs, actual $($notice.height/$density) DIPs"}
  }
  $appName=Catalog-Text 'ja' 'common-app-name' 'common'
  if(!$root.Current.Name.EndsWith(' · '+$appName)){throw 'The retained window title did not use the current app name'}
  $bitmap=$null;$graphics=$null;$dpi=[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))
  try{
   $owner=[uint32]0;[CapyRowPointer]::GetWindowThreadProcessId([IntPtr]$first.hwnd,[ref]$owner)|Out-Null
   if($owner -ne $review.Id -or $root.Current.ProcessId -ne $review.Id -or [CapyRowPointer]::GetForegroundWindow() -ne [IntPtr]$first.hwnd){throw 'The notice capture lost its owned foreground window'}
   $timing.capture_guard_start_ms=$clock.Elapsed.TotalMilliseconds;$frame=Find 'canvas-notice' -Visible;$windowBounds=$root.Current.BoundingRectangle
   $timing.capture_guard_ms=$clock.Elapsed.TotalMilliseconds;$timing.frame_found=[bool]$frame;$timing.window_bounds=$windowBounds.ToString();$timing.frame_bounds=$(if($frame){$frame.Current.BoundingRectangle.ToString()}else{$null})
   if(!$frame -or !$windowBounds.Contains($frame.Current.BoundingRectangle)){throw 'The notice is not fully visible in its owned capture window'}
   $bitmap=[Drawing.Bitmap]::new([int]$windowBounds.Width,[int]$windowBounds.Height);$graphics=[Drawing.Graphics]::FromImage($bitmap)
   $graphics.CopyFromScreen([int]$windowBounds.X,[int]$windowBounds.Y,0,0,$bitmap.Size)
   $timing.capture_copy_completed_ms=$clock.Elapsed.TotalMilliseconds
   if(!(Find 'canvas-notice' -Visible)){throw 'The notice expired before its composed capture completed'}
   Wait-Until {$view=Model;$view -and !$view.state.notice} 'Relabeling restarted the original notice deadline' 4
   if($clock.Elapsed.TotalSeconds -gt 4.8){throw "Relabeling extended the notice deadline to $($clock.Elapsed.TotalSeconds) seconds"}
   $bitmap.Save((Join-Path $run ("notice-localized-$caseName.png")),[Drawing.Imaging.ImageFormat]::Png)
   $updated|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ("notice-localized-$caseName.json"))
   $notice.layout|ConvertTo-Json -Depth 6|Set-Content (Join-Path $run ("notice-layout-$caseName.json"))
  }finally{if($graphics){$graphics.Dispose()};if($bitmap){$bitmap.Dispose()};[CapyWindowApi]::SetThreadDpiAwarenessContext($dpi)|Out-Null;$timing.final_ms=$clock.Elapsed.TotalMilliseconds;$timing|ConvertTo-Json|Set-Content (Join-Path $run ("notice-timing-$caseName.json"))}
  if($locked){(Control 'layer-lock').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle();Wait-Until {!(Model).state.layer_tools.editing_layer.locked} 'The image layer did not unlock'}
 }
}
