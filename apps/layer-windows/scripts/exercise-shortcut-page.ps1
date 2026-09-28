param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/shortcut-page/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{}
function Owned([string]$Value){
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Value),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Prefs{(Model).preferences}
function Page{(Prefs).shortcut_page}
function Shown([string]$Id){$item=Find $Id;if($item -and !$item.Current.IsOffscreen){$item}}
function Press([uint16]$Key){[CapyRowPointer]::Key([uint32]$review.Id,$Key)}
function Choose([string]$Id,[string]$Name){
 (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 $hit=@{item=$null};Wait-Until {$hit.item=Owned $Name;$null -ne $hit.item} "Missing choice $Name"
 $hit.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Type-Into([string]$Id,[string]$Text){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)}
try {
 Enter-CapyEnvironment
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned shortcut page review $($review.Id): $run"
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do{$review.Refresh();if($review.HasExited){throw 'Review exited at startup'};Start-Sleep -Milliseconds 100}while($review.MainWindowHandle -eq [IntPtr]::Zero -and $watch.Elapsed.TotalSeconds -lt 45)
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Wait-Until {(Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Shortcut page review did not start' 45
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 (Control 'Drawing canvas' -Name).SetFocus()
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {(Prefs) -and (Shown 'CloseButton')} 'Ctrl+, did not open Preferences'
 Invoke 'Keyboard Shortcuts' -Name
 Wait-Until {(Prefs).page -eq 'shortcuts' -and (Shown 'shortcut-category-Modifier keys') -and (Shown 'keymap-preset')} 'Keyboard Shortcuts did not list its categories before any search'
 if((Page).filtering){throw 'The shortcut page opened filtered'}
 foreach($category in (Page).categories){if(!(Find ('shortcut-category-'+$category.id))){throw "Category $($category.id) is not listed"}}
 Capture 'shortcuts-root-dark'
 $checks.categories_on_open='passed'

 $category=@((Page).categories)[1].id
 Invoke-Id ('shortcut-category-'+$category)
 Wait-Until {(Page).category -eq $category -and (Find 'settings-title').Current.Name -eq $category -and (Shown 'shortcut-category-back')} "Category $category did not open with its title and Back"
 $first=@((Prefs).shortcuts|Where-Object visible)[0]
 Wait-Until {Shown ('shortcut-'+$first.id)} "Category row $($first.id) is missing"
 Capture 'shortcuts-category-dark'
 Press 0x1B
 Wait-Until {!(Page).category -and (Prefs)} 'Escape did not return from the category'
 $checks.category_navigation_and_escape='passed'

 $search=Control 'shortcuts-search';$search.SetFocus();Wait-Until {$search.Current.HasKeyboardFocus} 'Shortcut search did not take focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {(Prefs).shortcut_query -and @((Prefs).shortcuts|Where-Object {$_.id -eq 'command.Undo' -and $_.visible}).Count -eq 1 -and (Shown 'shortcut-command.Undo')} 'A pressed Ctrl+Z did not find Undo'
 $checks.chord_search='passed'

 Invoke-Id 'shortcut-command.Undo'
 Wait-Until {(Prefs).shortcut_editor.id -eq 'command.Undo' -and (Shown 'shortcut-editor-close') -and (Shown 'add-shortcut')} 'The Undo editor sheet did not open'
 Invoke-Id 'add-shortcut'
 Wait-Until {(Prefs).capture -and (Shown 'shortcut-recording')} 'Add Shortcut did not start recording'
 Press 0x78
 Wait-Until {(Prefs).capture.shortcut -eq 'F9' -and (Control 'confirm-shortcut').Current.IsEnabled} 'Recording did not capture F9'
 Capture 'shortcut-recording-dark'
 Invoke-Id 'confirm-shortcut'
 Wait-Until {!(Prefs).capture -and @((Prefs).shortcut_editor.bindings) -contains 'F9' -and (Prefs).shortcut_editor.modified} 'Add did not bind F9'
 $index=[array]::IndexOf(@((Prefs).shortcut_editor.bindings),'F9')
 Invoke-Id ('remove-shortcut-'+$index)
 Wait-Until {@((Prefs).shortcut_editor.bindings) -notcontains 'F9' -and !(Prefs).shortcut_editor.modified} 'Removing F9 did not restore the default'
 Press 0x1B
 Wait-Until {!(Prefs).shortcut_editor -and (Prefs)} 'Escape did not close the editor sheet'
 Type-Into 'shortcuts-search' ''
 Wait-Until {!(Page).filtering} 'Clearing the search did not return to categories'
 $checks.editor_record_add_remove='passed'

 $original=(Prefs).keymap.selected
 Choose 'keymap-preset' 'Krita Style'
 Wait-Until {(Prefs).keymap.selected -eq 'krita'} 'Choosing Krita Style did not select its keymap'
 Invoke-Id 'keymap-menu'
 Wait-Until {$null -ne (Owned 'Differences…')} 'The keymap menu did not open'
 (Owned 'Differences…').GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Prefs).keymap.details -and (Shown 'keymap-details-close')} 'Differences did not open'
 Capture 'keymap-details-dark'
 Press 0x1B
 Wait-Until {!(Prefs).keymap.details -and (Prefs)} 'Escape did not close Differences'
 Choose 'keymap-preset' (@((Prefs).keymap.presets|Where-Object id -eq $original)[0].title)
 Wait-Until {(Prefs).keymap.selected -eq $original} 'The original keymap was not restored'
 $checks.keymap_preset_and_differences='passed'

 Invoke-Id 'shortcut-category-Modifier keys'
 Wait-Until {(Page).category -eq 'Modifier keys' -and (Shown 'add-modifier-key')} 'Modifier keys did not open'
 $modifier=@((Page).modifiers|Where-Object visible)[0]
 Invoke-Id ('modifier-'+$modifier.label)
 Wait-Until {(Prefs).modifier_editor.label -eq $modifier.label -and (Shown 'modifier-same')} "Modifier key $($modifier.label) did not open"
 $perTool=(Prefs).modifier_editor.per_tool
 (Control 'modifier-same').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Prefs).modifier_editor.per_tool -ne $perTool} 'Same for every tool did not change'
 $action=@((Prefs).modifier_editor.actions)[0];$suffix=if($action.category){$action.category}else{'all'}
 Invoke-Id ('modifier-action-'+$suffix)
 Wait-Until {(Page).picker -and (Shown 'action-picker-search')} 'The action picker did not open'
 Type-Into 'action-picker-search' 'nothing'
 Wait-Until {(Page).picker.query -eq 'nothing' -and (Shown 'action-nothing')} 'Searching the picker did not offer Nothing'
 Capture 'action-picker-dark'
 Invoke-Id 'action-nothing'
 Wait-Until {!(Page).picker -and (Prefs).modifier_editor.modified} 'Choosing Nothing did not change the modifier key'
 Invoke-Id 'modifier-reset'
 Wait-Until {!(Prefs).modifier_editor.modified} 'Reset did not restore the modifier key'
 Press 0x1B;Wait-Until {!(Prefs).modifier_editor -and (Page).category -eq 'Modifier keys'} 'Escape did not return from the modifier key'
 Press 0x1B;Wait-Until {!(Page).category -and (Prefs)} 'Escape did not return to the root page'
 $checks.modifier_key_picker_reset='passed'

 Invoke 'Pen & Input' -Name
 Wait-Until {(Prefs).page -eq 'input' -and (Find 'trigger-touch.tap.2') -and (Find 'trigger-pen.button.primary')} 'Pen & Input did not list finger taps and the pen button'
 if(Find 'trigger-pen.button.secondary'){throw 'Windows listed an upper pen button that Windows Ink cannot report'}
 $eraser=@(((Prefs).pages|Where-Object id -eq 'input').groups.rows|Where-Object {$_.id -match 'eraser' -and $_.visible -ne $false})
 if($eraser.Count -lt 2){throw 'The eraser end rows are hidden'}
 Invoke-Id 'trigger-touch.tap.2'
 Wait-Until {(Page).picker.trigger -eq 'touch.tap.2' -and (Shown 'action-nothing')} 'The two-finger tap picker did not open'
 Invoke-Id 'action-nothing'
 Wait-Until {!(Page).picker -and $null -ne (Model).state.settings.gestures -and (Model).state.settings.gestures.'touch.tap.2' -eq ''} 'Nothing did not turn off the two-finger tap'
 Invoke-Id 'trigger-touch.tap.2'
 Wait-Until {(Page).picker.modified -and (Shown 'action-picker-reset')} 'The picker did not offer Reset'
 Invoke-Id 'action-picker-reset'
 Wait-Until {!(Model).state.settings.gestures -or $null -eq (Model).state.settings.gestures.'touch.tap.2'} 'Reset did not restore the two-finger tap'
 Wait-Until {!(Page).picker -and (Prefs)} 'Reset did not close the picker'
 Invoke-Id 'trigger-pen.button.primary'
 Wait-Until {(Prefs).pen_button_editor -and (Shown 'pen-button-same')} 'The pen button page did not open'
 Capture 'pen-button-dark'
 Press 0x1B;Wait-Until {!(Prefs).pen_button_editor -and (Prefs)} 'Escape did not return from the pen button page'
 $checks.pen_and_input_triggers='passed'

 Press 0x1B
 Wait-Until {!(Prefs)} 'Escape on the root page did not close Preferences'
 $checks.escape_closes_preferences='passed'

 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
