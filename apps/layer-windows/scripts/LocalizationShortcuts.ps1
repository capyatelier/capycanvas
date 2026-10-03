function Native-Identities([string[]]$Ids){
 $result=@{};foreach($id in $Ids){$result[$id]=(Control $id).GetRuntimeId() -join ':'};$result
}
function Check-Identities($Identities){
 foreach($id in $Identities.Keys){if(((Control $id).GetRuntimeId() -join ':') -ne $Identities[$id]){throw "Locale publication replaced a retained native shortcut control: $id"}}
}
function Shortcut-Owner($View){
 if(($View.state.document_file|ConvertTo-Json -Depth 20 -Compress) -ne $shortcutDocument -or $View.windows_gpu_generation -ne $shortcutGpu){throw 'Shortcut publication changed document history or GPU ownership'}
 $settings=Read-Snapshot (Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json')
 if(($settings|Select-Object shortcuts,hold_keys,pen_buttons,keymap|ConvertTo-Json -Depth 30 -Compress) -ne $shortcutSettings){throw 'Locale publication changed stored shortcut or modifier semantics'}
}
function Shortcut-Surfaces{
 Use-Window $first
 if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{Menu-Command 'edit' 'settings'}
 Invoke-Id 'preference-page-shortcuts'
 Wait-Until {$view=Model;$view -and $view.preferences.page -eq 'shortcuts' -and (Find 'shortcuts-search')} 'Retained Shortcuts did not open' 30
 $shortcutDocument=(Fresh-Model).state.document_file|ConvertTo-Json -Depth 20 -Compress;$shortcutGpu=(Fresh-Model).windows_gpu_generation
 $shortcutSettings=Read-Snapshot (Join-Path $env:CAPY_SETTINGS_DIRECTORY 'settings.json')|Select-Object shortcuts,hold_keys,pen_buttons,keymap|ConvertTo-Json -Depth 30 -Compress
 $search=Control 'shortcuts-search';Select-Draft $search $literalName
 $filterIds=Native-Identities @('shortcuts-search','shortcut-context','shortcut-show','keymap-preset','keymap-menu')
 $contextOption=(Selected-Option (Control 'shortcut-context')).GetRuntimeId() -join ':';$showOption=(Selected-Option (Control 'shortcut-show')).GetRuntimeId() -join ':'
 $keymapSelected=(Fresh-Model).preferences.keymap.selected;$presetOption=(Selected-Option (Control 'keymap-preset')).GetRuntimeId() -join ':'
 Surface-Languages 'retained-shortcut-search' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $filterIds
  if($view.preferences.shortcut_query -ne $literalName -or (Value (Control 'shortcuts-search')) -ne $literalName -or (Selection (Control 'shortcuts-search')) -ne $literalName){throw 'Shortcut search lost its literal Unicode draft or selection'}
  $page=$view.preferences.shortcut_page
  $keymap=$view.preferences.keymap;$preset=@($keymap.presets|Where-Object id -eq $keymapSelected)[0]
  if($keymap.selected -ne $keymapSelected -or (Selected-Option (Control 'keymap-preset')).Current.Name -ne $preset.title -or ((Selected-Option (Control 'keymap-preset')).GetRuntimeId() -join ':') -ne $presetOption){throw 'Keymap preset lost its retained option identity, semantic selection or current caption'}
  if($null -ne $page.context -or $page.show -ne 'all' -or (Selected-Option (Control 'shortcut-context')).Current.Name -ne $page.contexts[0].label -or (Selected-Option (Control 'shortcut-show')).Current.Name -ne $page.shows[0].label -or ((Selected-Option (Control 'shortcut-context')).GetRuntimeId() -join ':') -ne $contextOption -or ((Selected-Option (Control 'shortcut-show')).GetRuntimeId() -join ':') -ne $showOption){throw 'Shortcut filter options lost their retained identity, value or current caption'}
 }
 (Control 'shortcuts-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('')
 Wait-Until {$view=Model;$view -and $view.preferences.shortcut_query -eq '' -and (Find 'shortcut-category-Modifier keys')} 'Shortcut categories did not return after clearing search' 15
 Invoke-Id 'shortcut-category-Modifier keys'
 Wait-Until {(Find 'modifier-alt')} 'Semantic modifier row did not open' 15
 Invoke-Id 'modifier-alt'
 Wait-Until {$view=Model;$view -and $view.preferences.modifier_editor -and (Find 'modifier-same')} 'Retained modifier editor did not open' 15
 $modifierBefore=(Fresh-Model).preferences.modifier_editor
 $modifierIds=Native-Identities @('modifier-same','modifier-remove','modifier-hold-summary')
 $actionIds=@($modifierBefore.actions|ForEach-Object {if($null -eq $_.category){'modifier-action-all'}else{'modifier-action-'+$_.category}})
 foreach($id in $actionIds){$modifierIds[$id]=(Control $id).GetRuntimeId() -join ':'}
 Surface-Languages 'retained-modifier-editor' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $modifierIds;$editor=$view.preferences.modifier_editor
  if(($editor.key|ConvertTo-Json -Compress) -ne ($modifierBefore.key|ConvertTo-Json -Compress) -or $editor.per_tool -ne $modifierBefore.per_tool -or $editor.modified -ne $modifierBefore.modified){throw 'Locale publication changed the retained modifier binding'}
  $help=(Catalog-Text $choice.tag 'settings-modifier-hold-help' 'settings').Replace('{ $label }',$editor.label)
  $expectedToggle=if($editor.per_tool){[System.Windows.Automation.ToggleState]::Off}else{[System.Windows.Automation.ToggleState]::On}
  if((Control 'modifier-same').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne $expectedToggle){throw 'Modifier per-tool toggle lost its retained semantic state'}
  foreach($action in $editor.actions){$id=if($null -eq $action.category){'modifier-action-all'}else{'modifier-action-'+$action.category};if((Control $id).Current.Name -ne $action.label -or (Control $id).Current.ItemStatus -ne $action.action){throw 'Modifier action row did not use current shared labels/status'}}
  if((Control 'modifier-hold-summary').Current.Name -ne $help -or (Control 'modifier-same').Current.Name -ne (Catalog-Text $choice.tag 'native-shortcuts-same-all-tools' 'common')){throw 'Modifier editor copy did not follow the shared current language'}
 }
 Invoke-Id $actionIds[0]
 Wait-Until {$view=Model;$view -and $view.preferences.shortcut_page.picker -and (Find 'action-picker-search')} 'Retained modifier action picker did not open' 15
 $pickerTrigger=(Fresh-Model).preferences.shortcut_page.picker.trigger
 $pickerBefore=(Fresh-Model).preferences.shortcut_page.picker
 $pickerActionIds=@($pickerBefore.sections|ForEach-Object {$_.actions}|ForEach-Object {'action-'+$_.id})
 if($pickerBefore.nothing_visible){$pickerActionIds+='action-nothing'}
 $pickerActionIdentities=Native-Identities $pickerActionIds
 $pickerSelected=@($pickerBefore.sections|ForEach-Object {$_.actions}|Where-Object selected|ForEach-Object id) -join '|'
 Surface-Languages 'retained-action-options' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $pickerActionIdentities;$picker=$view.preferences.shortcut_page.picker
  if(!$picker -or $picker.trigger -ne $pickerTrigger -or (@($picker.sections|ForEach-Object {$_.actions}|Where-Object selected|ForEach-Object id) -join '|') -ne $pickerSelected -or $picker.nothing -ne $pickerBefore.nothing){throw 'Locale publication changed the retained action choice'}
  foreach($action in @($picker.sections|ForEach-Object {$_.actions})){if((Control ('action-'+$action.id)).Current.Name -ne $action.label){throw 'Action option caption did not use the current shared copy'}}
 }
 Select-Draft (Control 'action-picker-search') $literalName
 $pickerIds=Native-Identities @('action-picker','action-picker-search','action-picker-description','action-picker-close')
 Surface-Languages 'retained-action-picker' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $pickerIds;$picker=$view.preferences.shortcut_page.picker
  if(!$picker -or $picker.trigger -ne $pickerTrigger -or $picker.query -ne $literalName -or (Value (Control 'action-picker-search')) -ne $literalName -or (Selection (Control 'action-picker-search')) -ne $literalName){throw 'Action picker lost its retained trigger, Unicode search draft or selection'}
  if((Control 'action-picker-search').Current.Name -ne (Catalog-Text $choice.tag 'native-shortcuts-search-actions' 'common') -or (Control 'action-picker-description').Current.Name -ne $picker.description){throw 'Action picker copy did not follow the shared current language'}
 }
 Invoke-Id 'action-picker-close'
 Wait-Until {$view=Model;$view -and !$view.preferences.shortcut_page.picker -and $view.preferences.modifier_editor -and !(Find 'action-picker-search')} 'Action picker did not return to its modifier editor' 15
 Invoke-Id 'shortcut-category-back'
 Wait-Until {$view=Model;$view -and !$view.preferences.modifier_editor -and $view.preferences.shortcut_page.category -eq 'Modifier keys' -and (Find 'modifier-alt')} 'Modifier editor did not return to its category' 15
 Invoke-Id 'shortcut-category-back'
 Wait-Until {$view=Model;$view -and $null -eq $view.preferences.shortcut_page.category -and (Find 'shortcut-category-Edit')} 'Shortcut categories did not return after closing the modifier editor' 15
 Invoke-Id 'shortcut-category-Edit'
 Wait-Until {$view=Model;$view -and @($view.preferences.shortcuts|Where-Object {$_.visible -and $_.bindings.Count -gt 0}).Count} 'Assigned Edit shortcuts did not become visible' 15
 $shortcut=@((Fresh-Model).preferences.shortcuts|Where-Object {$_.visible -and $_.bindings.Count -gt 0})[0]
 Invoke-Id ('shortcut-'+$shortcut.id)
 Wait-Until {$view=Model;$view -and $view.preferences.shortcut_editor -and (Find 'shortcut-default-summary')} 'Retained shortcut editor did not open' 15
 $shortcutBefore=(Fresh-Model).preferences.shortcut_editor
 $editorIds=Native-Identities @('shortcut-editor','shortcut-editor-close','shortcut-default-summary','remove-shortcut-0')
 if($shortcutBefore.can_add){$editorIds['add-shortcut']=(Control 'add-shortcut').GetRuntimeId() -join ':'}
 Surface-Languages 'retained-shortcut-editor' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $editorIds;$editor=$view.preferences.shortcut_editor
  if($editor.id -ne $shortcutBefore.id -or $editor.keys.Count -ne $shortcutBefore.keys.Count -or $editor.modified -ne $shortcutBefore.modified -or $editor.can_add -ne $shortcutBefore.can_add){throw 'Locale publication changed the retained shortcut binding or editor state'}
  $summary=if($editor.defaults.Count){(Catalog-Text $choice.tag 'settings-shortcut-default' 'settings').Replace('{ $keys }',($editor.defaults -join ' / '))}else{Catalog-Text $choice.tag 'settings-shortcut-default-empty' 'settings'}
  foreach($overlap in $editor.overlaps){$summary+="`n"+$overlap}
  if((Control 'shortcut-default-summary').Current.Name -ne $summary){throw 'Shortcut default summary did not use the current shared copy'}
 }
 if(!$shortcutBefore.can_add){throw 'Assigned shortcut fixture cannot exercise a retained capture draft'}
 Invoke-Id 'add-shortcut'
 Wait-Until {$view=Model;$view -and $view.preferences.capture -and (Find 'shortcut-recording')} 'Shortcut capture did not open' 15
 Key 0x78
 Wait-Until {$view=Model;$view -and $view.preferences.capture.chord -and (Control 'confirm-shortcut').Current.IsEnabled} 'Shortcut capture did not retain the injected F9 draft' 15
 $captureBefore=(Fresh-Model).preferences.capture;$captureChord=$captureBefore.chord|ConvertTo-Json -Depth 10 -Compress
 $captureIds=Native-Identities @('shortcut-recording','cancel-shortcut','confirm-shortcut')
 Surface-Languages 'retained-shortcut-capture' {
  $view=Fresh-Model $first $choice.tag;Shortcut-Owner $view;Check-Identities $captureIds;$capture=$view.preferences.capture
  if(!$capture -or $capture.id -ne $captureBefore.id -or ($capture.chord|ConvertTo-Json -Depth 10 -Compress) -ne $captureChord -or $capture.existing -ne $captureBefore.existing -or !(Control 'confirm-shortcut').Current.IsEnabled){throw 'Locale publication changed the pending native shortcut capture'}
  if((Control 'shortcut-recording').Current.Name -ne $capture.shortcut){throw 'Shortcut recording caption did not follow the shared current key presentation'}
  $confirmation=if($capture.existing){'native-shortcuts-open'}elseif($null -ne $capture.conflict){'native-shortcuts-reassign'}else{'native-shortcuts-add'}
  if((Control 'cancel-shortcut').Current.Name -ne (Catalog-Text $choice.tag 'common-cancel' 'common') -or (Control 'confirm-shortcut').Current.Name -ne (Catalog-Text $choice.tag $confirmation 'common')){throw 'Shortcut capture buttons did not use the current canonical action copy'}
 }
 Invoke-Id 'cancel-shortcut';Wait-Until {$view=Model;$view -and !$view.preferences.capture -and $view.preferences.shortcut_editor} 'Shortcut capture did not cancel without committing its draft' 15
 Invoke-Id 'shortcut-editor-close';Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.preferences} 'Retained Shortcut Preferences did not close' 15
 Use-Window $textWindow
}
