param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=20;$CapyPopups=$true
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public sealed class CapyShortcutLayout : IDisposable {
 [DllImport("user32.dll")] static extern IntPtr GetKeyboardLayout(uint thread);
 [DllImport("user32.dll")] static extern int GetKeyboardLayoutList(int count,[Out] IntPtr[] layouts);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr LoadKeyboardLayout(string name,uint flags);
 [DllImport("user32.dll")] static extern bool UnloadKeyboardLayout(IntPtr layout);
 [DllImport("user32.dll")] static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr window,uint message,IntPtr wparam,IntPtr lparam);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int ToUnicodeEx(uint key,uint scan,byte[] state,StringBuilder text,int count,uint flags,IntPtr layout);
 readonly IntPtr window,previous,layout; readonly uint thread; readonly bool loaded;
 public CapyShortcutLayout(IntPtr window) {
  this.window=window;uint process;thread=GetWindowThreadProcessId(window,out process);previous=GetKeyboardLayout(thread);
  var existing=new IntPtr[GetKeyboardLayoutList(0,null)];GetKeyboardLayoutList(existing.Length,existing);
  layout=LoadKeyboardLayout("00000419",0);loaded=Array.IndexOf(existing,layout)<0;
  try {
   if(layout==IntPtr.Zero)throw new InvalidOperationException("Russian keyboard layout unavailable");
   SendMessage(window,0x50,IntPtr.Zero,layout);
   var text=new StringBuilder(8);
   if(GetKeyboardLayout(thread)!=layout||ToUnicodeEx(0x5A,0x2C,new byte[256],text,8,4,layout)!=1||text[0]!=0x44F)
    throw new InvalidOperationException("Owned app did not activate Russian Z");
  } catch { Dispose();throw; }
 }
 public void Dispose() {
  try {
   if(IsWindow(window)){
    SendMessage(window,0x50,IntPtr.Zero,previous);
    if(GetKeyboardLayout(thread)!=previous)throw new InvalidOperationException("Owned app keyboard layout was not restored");
   }
  } finally { if(loaded&&layout!=IntPtr.Zero&&!UnloadKeyboardLayout(layout))throw new InvalidOperationException("Fixture Russian keyboard layout was not unloaded"); }
 }
}
'@
Add-Type -AssemblyName System.Drawing
$inkHelpers=[System.Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'capture-number-controls.ps1'),[ref]$null,[ref]$null)
foreach($helper in @('Measure-RenderedInk','Assert-ControlCaptionInk','Save-ControlCaptionInk')){
 $definition=$inkHelpers.Find({param($node)$node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $helper},$true)
 if(!$definition){throw "Missing rendered ink helper: $helper"}
 . ([scriptblock]::Create($definition.Extent.Text))
}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/shortcut-page/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$CapyTraceDirectory=$run
$checks=[ordered]@{theme=$Theme}
function Owned([string]$Value){
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Value),
  [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
 [System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Prefs{(Model).preferences}
function Page{(Prefs).shortcut_page}
function Shown([string]$Id){$item=Find $Id;if($item -and !$item.Current.IsOffscreen){$item}}
$compactSwitchChecks=0
function Visible-NamedSwitch([string]$Name){
 $condition=[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsTogglePatternAvailableProperty,$true))
 @($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)|Where-Object {!$_.Current.IsOffscreen -and !$_.Current.BoundingRectangle.IsEmpty -and $_.Current.BoundingRectangle.Width -gt 0})
}
function Press([uint16]$Key){[CapyRowPointer]::Key([uint32]$review.Id,$Key)}
function Choose([string]$Id,[string]$Name){
 (Control $Id).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 $hit=@{item=$null};Wait-Until {$hit.item=Owned $Name;$null -ne $hit.item} "Missing choice $Name"
 $hit.item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Type-Into([string]$Id,[string]$Text){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)}
function Visit-ProfileManager([string]$CaptureName){
 Invoke-Id 'manage-color-profiles'
 $done=@{item=$null}
 Wait-Until {
  $condition=[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Done'),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty,$review.Id))
  $done.item=[System.Windows.Automation.AutomationElement]::RootElement.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
  !(Prefs) -and $done.item
 } 'Manage Color Profiles did not open the profile manager'
 $done.item=Control 'Done' -Name -Type ([System.Windows.Automation.ControlType]::Button) -Arranged
 if($CaptureName){Capture $CaptureName -WithModel -Composed}
 $done.item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
 Wait-Until {(Prefs).page -eq 'color' -and (Shown 'CloseButton')} 'Closing the profile manager did not return to the Color page'
}
try {
 Enter-CapyEnvironment
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Compress))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle
 Write-Output "Owned shortcut page review $($review.Id): $run"
 [CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
 $native=@{window=$null}
 Wait-Until {$native.window=Owned-DrawingWindow $review;$native.window -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Shortcut page review did not start' 45
 $root=$native.window.Root;$drawingWindow=$native.window.Handle
 $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
 [CapyRowPointer]::SetForegroundWindow($drawingWindow)|Out-Null
 (Control 'drawing-canvas').SetFocus()
 $layers=(Model).state.layers.Count
 for($i=1;$i -le 2;$i++){
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),[uint16]0x4E)
  Wait-Until {(Model).state.layers.Count -eq $layers+$i} 'Seed layer shortcut did not finish'
 }
 $russian=[CapyShortcutLayout]::new($drawingWindow)
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {(Model).state.layers.Count -eq $layers+1} 'Russian Ctrl+Z must undo exactly one layer'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11,0x10),[uint16]0x5A)
 Wait-Until {(Model).state.layers.Count -eq $layers+2} 'Russian Ctrl+Shift+Z must redo exactly one layer'
 foreach($tool in @(@{key=0x45;id='eraser'},@{key=0x42;id='brush'})){
  Press $tool.key
  Wait-Until {@((Model).state.commands|Where-Object id -eq $tool.id)[0].selected} 'Russian tool key did not select the tool'
 }
 $checks.russian_canvas_shortcuts='Undo/Redo once and B selects Paint'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0xBC)
 Wait-Until {(Prefs) -and (Shown 'CloseButton')} 'Ctrl+, did not open Preferences'
 $originalTransparency=(Model).state.settings.transparency
 foreach($glass in @(@{name='off';index=0},@{name='low';index=1})){
  Set-CapyTransparency $glass.name
  foreach($page in @((Prefs).pages)){
   Invoke-Id ('preference-page-'+$page.id)
   Wait-Until {(Prefs).page -eq $page.id -and (Shown 'settings-title')} "Preferences page $($page.id) did not open"
   $name="preferences-$($page.id)-$Theme-glass-$($glass.name)"
   foreach($row in @($page.groups.rows|Where-Object {$_.kind.type -eq 'switch' -and $_.visible -ne $false})){
    $switch=@(Visible-NamedSwitch $row.title)
    if($switch.Count -gt 1){throw "Preferences switch $($row.id) has ambiguous arranged peers"}
    if($switch.Count){Assert-CapyCompactSwitch $switch[0] ([bool]$row.kind.active) $drawingWindow (Join-Path $run ($name+'-'+$row.id+'-compact-switch.json'));$compactSwitchChecks++}
   }
   Capture $name -WithModel -Composed
   if($page.id -eq 'color'){
    Visit-ProfileManager ($name+'-profile-manager')
   }
   if($page.id -eq 'appearance'){
    $dpi=[CapyRowPointer]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)/96.
    foreach($row in @($page.groups.rows|Where-Object {$_.kind.type -eq 'swatches' -and $_.kind.inline -and $_.visible -ne $false})){
     $swatches=@(0..($row.kind.swatches.Count-1)|ForEach-Object {Shown ('setting-'+$row.id+'-swatch-'+$_)})
     for($i=1;$i -lt $swatches.Count;$i++){
      $pitch=($swatches[$i].Current.BoundingRectangle.Left-$swatches[$i-1].Current.BoundingRectangle.Left)/$dpi
      if([Math]::Abs($pitch-38) -gt 1){throw "Inline swatch $($row.id) pitch is $pitch DIP instead of 38"}
     }
    }
   }
   if($page.id -eq 'shortcuts'){
    Type-Into 'shortcuts-search' 'Undo'
    Wait-Until {(Prefs).shortcut_query -eq 'Undo' -and (Page).filtering -and (Shown 'shortcut-command.Undo')} 'Preferences capture search did not filter Undo'
    Capture ($name+'-filtered') -WithModel -Composed
    $beforeContext=(Page).context|ConvertTo-Json -Depth 10 -Compress;$beforeShow=(Page).show
    foreach($filter in @(@{id='shortcut-context';items=@((Page).contexts)},@{id='shortcut-show';items=@((Page).shows)})){
     $picker=Control $filter.id -Arranged
     $pattern=$picker.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
     $pattern.Expand()
     Wait-Until {$pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Expanded} 'Shortcut filter did not expand'
     $null=Control ($filter.items[0].label) -Name -Type ([System.Windows.Automation.ControlType]::ListItem) -Arranged
     Capture ($name+'-'+$filter.id+'-open') -WithModel -Composed
     $pattern.Collapse()
     Wait-Until {$pattern.Current.ExpandCollapseState -eq [System.Windows.Automation.ExpandCollapseState]::Collapsed} 'Shortcut filter did not collapse'
     if(((Page).context|ConvertTo-Json -Depth 10 -Compress) -cne $beforeContext -or (Page).show -ne $beforeShow -or (Prefs).shortcut_query -ne 'Undo'){throw 'Opening a shortcut filter changed its shared selection or query'}
    }
    Type-Into 'shortcuts-search' ''
    Wait-Until {!(Page).filtering} 'Preferences capture search did not clear'
   }
   $metrics=@((Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)).FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {!$_.Current.IsOffscreen -and $_.Current.AutomationId}|ForEach-Object {
    $item=$_.Current;$bounds=$item.BoundingRectangle
    [ordered]@{id=$item.AutomationId;name=$item.Name;type=$item.ControlType.ProgrammaticName;x=$bounds.X;y=$bounds.Y;width=$bounds.Width;height=$bounds.Height}
   })
   $metrics|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ($name+'-metrics.json'))
  }
 }
 Set-CapyTransparency $originalTransparency
 if(!$compactSwitchChecks){throw 'Preferences did not expose an arranged compact switch'}
 $checks.preferences_pages_and_glass_captures='passed'
 Invoke 'Keyboard Shortcuts' -Name
 Wait-Until {(Prefs).page -eq 'shortcuts' -and (Shown 'shortcut-category-Modifier keys') -and (Shown 'keymap-preset')} 'Keyboard Shortcuts did not list its categories before any search'
 if((Page).filtering){throw 'The shortcut page opened filtered'}
 foreach($category in (Page).categories){if(!(Find ('shortcut-category-'+$category.id))){throw "Category $($category.id) is not listed"}}
 Capture "shortcuts-root-$Theme"
 $checks.categories_on_open='passed'

 $category=@((Page).categories)[1].id
 Invoke-Id ('shortcut-category-'+$category)
 Wait-Until {(Page).category -eq $category -and (Find 'settings-title').Current.Name -eq $category -and (Shown 'shortcut-category-back')} "Category $category did not open with its title and Back"
 $first=@((Prefs).shortcuts|Where-Object visible)[0]
 Wait-Until {Shown ('shortcut-'+$first.id)} "Category row $($first.id) is missing"
 Capture "shortcuts-category-$Theme"
 Press 0x1B
 Wait-Until {!(Page).category -and (Prefs)} 'Escape did not return from the category'
 $checks.category_navigation_and_escape='passed'

 $search=Control 'shortcuts-search';$search.SetFocus();Wait-Until {$search.Current.HasKeyboardFocus} 'Shortcut search did not take focus'
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x11),[uint16]0x5A)
 Wait-Until {(Prefs).shortcut_query -and @((Prefs).shortcuts|Where-Object {$_.id -eq 'command.Undo' -and $_.visible}).Count -eq 1 -and (Shown 'shortcut-command.Undo')} 'A pressed Ctrl+Z did not find Undo'
 Capture "shortcuts-filters-$Theme" -WithModel -Composed
 $checks.chord_search='passed'

 Invoke-Id 'shortcut-command.Undo'
 Wait-Until {(Prefs).shortcut_editor.id -eq 'command.Undo' -and (Shown 'shortcut-editor-close') -and (Shown 'add-shortcut')} 'The Undo editor sheet did not open'
 Invoke-Id 'add-shortcut'
 Wait-Until {(Prefs).capture -and (Shown 'shortcut-recording')} 'Add Shortcut did not start recording'
 Press 0x78
 Wait-Until {(Prefs).capture.shortcut -eq 'F9' -and (Control 'confirm-shortcut').Current.IsEnabled} 'Recording did not capture F9'
 Capture "shortcut-recording-$Theme"
 $bindingsBefore=ConvertTo-Json -InputObject ([string[]]@((Prefs).shortcut_editor.bindings)) -Compress
 Press 0x1B
 Wait-Until {!(Prefs).capture -and (Prefs).page -eq 'shortcuts' -and (Prefs).shortcut_editor.id -eq 'command.Undo' -and (Shown 'shortcut-editor-close') -and (Shown 'add-shortcut')} 'Escape did not cancel recording while keeping the shortcut editor and Preferences open'
 if((ConvertTo-Json -InputObject ([string[]]@((Prefs).shortcut_editor.bindings)) -Compress) -ne $bindingsBefore){throw 'Escape during capture changed the shortcut bindings'}
 $checks.capture_escape_cancels_recording_only='passed'
 Invoke-Id 'add-shortcut'
 Wait-Until {(Prefs).capture -and (Shown 'shortcut-recording')} 'Add Shortcut did not restart recording'
 Press 0x78
 Wait-Until {(Prefs).capture.shortcut -eq 'F9' -and (Control 'confirm-shortcut').Current.IsEnabled} 'Recording did not capture F9 after Escape'
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
 Capture "keymap-details-$Theme"
 Press 0x1B
 Wait-Until {!(Prefs).keymap.details -and (Prefs)} 'Escape did not close Differences'
 Choose 'keymap-preset' (@((Prefs).keymap.presets|Where-Object id -eq $original)[0].title)
 Wait-Until {(Prefs).keymap.selected -eq $original} 'The original keymap was not restored'
 $checks.keymap_preset_and_differences='passed'

 Invoke-Id 'shortcut-category-Modifier keys'
 Wait-Until {(Page).category -eq 'Modifier keys' -and (Shown 'add-modifier-key')} 'Modifier keys did not open'
 $modifier=@((Page).modifiers|Where-Object visible)[0]
 $held=$modifier.key
 Invoke-Id ('modifier-'+$held.key+$(if($held.command){'-command'})+$(if($held.shift){'-shift'})+$(if($held.alt){'-alt'}))
 Wait-Until {(Prefs).modifier_editor.label -eq $modifier.label -and (Shown 'modifier-same')} "Modifier key $($modifier.label) did not open"
 $perTool=(Prefs).modifier_editor.per_tool
 Assert-CapyCompactSwitch (Control 'modifier-same' -Arranged) (!$perTool) $drawingWindow (Join-Path $run ('modifier-same-'+$Theme+'-compact-switch.json'))
 (Control 'modifier-same').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
 Wait-Until {(Prefs).modifier_editor.per_tool -ne $perTool} 'Same for every tool did not change'
 Assert-CapyCompactSwitch (Control 'modifier-same' -Arranged) $perTool $drawingWindow (Join-Path $run ('modifier-same-toggled-'+$Theme+'-compact-switch.json'))
 $action=@((Prefs).modifier_editor.actions)[0];$suffix=if($action.category){$action.category}else{'all'}
 Invoke-Id ('modifier-action-'+$suffix)
 Wait-Until {(Page).picker -and (Shown 'action-picker-search')} 'The action picker did not open'
 Type-Into 'action-picker-search' 'nothing'
 Wait-Until {(Page).picker.query -eq 'nothing' -and (Shown 'action-nothing')} 'Searching the picker did not offer Nothing'
 Capture "action-picker-$Theme"
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
 $reset=Control 'action-picker-reset' -Arranged
 Capture "action-picker-reset-$Theme" -WithModel -Composed
 $null=Save-ControlCaptionInk $reset $drawingWindow (Join-Path $run "action-picker-reset-$Theme.png") (Model).state.palette.text (Join-Path $run "reset-caption-$Theme")
 Invoke-Id 'action-picker-reset'
 Wait-Until {!(Model).state.settings.gestures -or $null -eq (Model).state.settings.gestures.'touch.tap.2'} 'Reset did not restore the two-finger tap'
 Wait-Until {!(Page).picker -and (Prefs)} 'Reset did not close the picker'
 Invoke-Id 'trigger-pen.button.primary'
 Wait-Until {(Prefs).pen_button_editor -and (Shown 'pen-button-same')} 'The pen button page did not open'
 Capture "pen-button-$Theme"
 Press 0x1B;Wait-Until {!(Prefs).pen_button_editor -and (Prefs)} 'Escape did not return from the pen button page'
 $checks.pen_and_input_triggers='passed'

 Invoke 'Color' -Name -Within (Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))
 Wait-Until {(Prefs).page -eq 'color' -and (Shown 'manage-color-profiles')} 'The Color page did not offer Manage Color Profiles'
 Visit-ProfileManager
 $checks.profile_manager_returns_to_preferences='passed'

 Press 0x1B
 Wait-Until {!(Prefs)} 'Escape on the root page did not close Preferences'
 $checks.escape_closes_preferences='passed'

 $russian.Dispose();$russian=$null
 [CapyRowPointer]::Dispose()
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close -DiscardUnsaved -StateDirectory $run
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Native stderr requires review'}
 $checks.evidence=$run
 [pscustomobject]$checks|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
} catch {
 if($review -and !$review.HasExited){try{Capture 'failure' -WithModel}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
} finally {
 if($russian){$russian.Dispose()}
 [CapyRowPointer]::Dispose()
 if($review -and !$review.HasExited){Stop-Process -Id $review.Id -Force}
 Exit-CapyEnvironment
}
