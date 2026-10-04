param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','light-large')][string]$Theme='dark',[switch]$LargeText,[int]$LanguageLimit=0)
$ErrorActionPreference='Stop'
if($Theme -eq 'light-large'){$Theme='light';$LargeText=$true}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
. (Join-Path $PSScriptRoot 'LocalizationProfiles.ps1')
. (Join-Path $PSScriptRoot 'LocalizationShortcuts.ps1')
. (Join-Path $PSScriptRoot 'LocalizationProperties.ps1')
Add-Type -AssemblyName System.Drawing
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$run=Join-Path $repo ('artifacts/windows/localization/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
$completed=$false
$numericDraft='１２＋3 {draft}'
$literalName='Tiếng Việt Tiếng Việt İı ไทย 🎨 {draft}'
Add-Type -TypeDefinition @'
using System;using System.Runtime.InteropServices;
public static class CapyLocaleLayout {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessageTimeout(IntPtr window,uint message,UIntPtr w,string l,uint flags,uint timeout,out UIntPtr result);
 public static void NotifyTextSize(){UIntPtr result;SendMessageTimeout(new IntPtr(0xffff),0x1a,UIntPtr.Zero,"Accessibility",2,2000,out result);}
}
'@
$textSizePath='HKCU:\Software\Microsoft\Accessibility'
$textSizeSaved=$null;$textSizeChanged=$false
function Text-Scale{
 $expression='[Windows.UI.ViewManagement.UISettings,Windows.UI.ViewManagement,ContentType=WindowsRuntime]::new().TextScaleFactor'
 [double](& powershell.exe -NoProfile -NonInteractive -Command $expression)
}
function Resize-Window($Window,[int]$Width,[int]$Height){
 Use-Window $Window
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $Window.hwnd -Action Resize -Width $Width -Height $Height
 Wait-Until {$actual=$root.Current.BoundingRectangle;[Math]::Abs($actual.Width-$Width) -lt 2 -and [Math]::Abs($actual.Height-$Height) -lt 2} 'Native window did not acknowledge its narrow dimensions' 15
}
function Dismiss-NarrowPreferences{
 $close=Control 'CloseButton' -Arranged;$bounds=$close.Current.BoundingRectangle
 $caption=(Fresh-Model).titlebar_insets[2]*([CapyRowPointer]::GetDpiForWindow([IntPtr]$script:current.hwnd)/96.)
 if($bounds.Top -lt $root.Current.BoundingRectangle.Top+$caption){throw 'Narrow Preferences dismissal overlaps the native titlebar'}
 $point=[Windows.Point]::new($bounds.X+$bounds.Width/2,$bounds.Y+$bounds.Height/2)
 $dpi=[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))
 try{
 $hit=[System.Windows.Automation.AutomationElement]::FromPoint($point)
 if(!$hit -or $hit.Current.ProcessId -ne $review.Id){throw 'The Preferences dismiss point is outside the owned app'}
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 [CapyRowPointer]::Down('mouse',[int]$point.X,[int]$point.Y);[CapyRowPointer]::Up()
 Wait-Until {$view=Model;$view -and !$view.preferences} 'A pointer click did not dismiss narrow Preferences' 15
 }finally{[CapyRowPointer]::Dispose();[CapyWindowApi]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
}

function Windows {try{(Read-Snapshot (Join-Path $run "windows-$($review.Id).json")).windows}catch{@()}}
function Model($Window=$script:current){
 $path=Join-Path $run "ui-state-$($review.Id)-$($Window.id).json"
 try{
  $snapshot=Read-Snapshot $path
  if($snapshot.process_id -ne $review.Id -or $snapshot.window_id -ne $Window.id -or !$snapshot.model.windows_isolated_settings){throw 'Snapshot owner/isolation guard rejected the parsed frame'}
  $snapshot.model
 }catch{
  $failure=$_
  if($script:localizationSnapshotsReady -and $script:modelReadFailureCount -lt 64){
   $script:modelReadFailureCount++;$probeBytes=0
   if([IO.File]::Exists($path)){try{$probe=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete));try{$copy=[IO.MemoryStream]::new();$probe.CopyTo($copy);$bytes=$copy.ToArray();$copy.Dispose()}finally{$probe.Dispose()};$probeBytes=$bytes.Length;[IO.File]::WriteAllBytes((Join-Path $run ("model-read-probe-$($script:modelReadFailureCount).json")),$bytes)}catch{}}
   [IO.File]::AppendAllText((Join-Path $run 'model-read-errors.log'),(@{utc=[DateTime]::UtcNow.ToString('o');path=$path;exception=$failure.Exception.ToString();hresult=$failure.Exception.HResult;later_probe_bytes=$probeBytes}|ConvertTo-Json -Compress)+[Environment]::NewLine)
  }
  $null
 }
}
function Fresh-Model($Window=$script:current,[string]$Tag){
 $hit=@{model=$null}
 Wait-Until {$hit.model=Model $Window;$hit.model -and $hit.model.state.document_file -and $null -ne $hit.model.windows_gpu_generation -and (!$Tag -or $hit.model.windows_active_tag -eq $Tag)} 'A complete owned snapshot did not become available for its acknowledged context' 10
 $hit.model
}
function Trace-Part([string]$Kind,[string]$Field,$Window=$script:current){
 try{
  $snapshot=Read-Snapshot (Join-Path $run "$Kind-$($review.Id)-$($Window.id).json")
  if($snapshot.process_id -eq $review.Id -and $snapshot.window_id -eq $Window.id){$snapshot.$Field}
 }catch{$null}
}
function Search{Trace-Part 'search-state' 'search'}
function Camera{Trace-Part 'camera-state' 'camera'}
function Use-Window($Window){
 $script:current=$Window;$script:root=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$Window.hwnd)
 Wait-Until {[CapyRowPointer]::SetForegroundWindow([IntPtr]$Window.hwnd)|Out-Null;[CapyRowPointer]::GetForegroundWindow() -eq [IntPtr]$Window.hwnd} 'Review window did not own foreground input' 10
}
function Focus-Canvas{
 Wait-Until {
  $canvas=Find 'drawing-canvas' -Visible
  if(!$canvas -or !$canvas.Current.IsEnabled -or !$canvas.Current.IsKeyboardFocusable){return $false}
  try{$canvas.SetFocus()}catch [System.Management.Automation.MethodInvocationException]{return $false}
  [System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq 'drawing-canvas'
 } 'Drawing canvas did not regain native keyboard focus' 30
}
function Ready($Window){Wait-Until {$view=Model $Window;$view.brush_ready -and $view.windows_workspace.ready} 'Window did not prepare' 120}
function Language-Row($Window=$script:current){@((Fresh-Model $Window).preferences.pages.groups.rows|Where-Object id -eq 'language')[0]}
function Request-Language([int]$Index){
 $row=Language-Row;$choice=$row.kind.options[$Index]
 $dropdown=(Control 'preference-choice-language').GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern)
 $dropdown.Expand()
 (Control $choice -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
 $dropdown.Collapse()
}
function Language-Choice([int]$Index){
 $choice=(Language-Row).kind.options[$Index];Request-Language $Index
 $expected=if($Index -eq 0){$systemTag}else{$shippedTags[$Index-1]};$ack=@{view=$null}
 Wait-Until {
  $ack.view=Model
  if(!$ack.view -or $null -eq $ack.view.windows_gpu_generation -or $ack.view.windows_active_tag -ne $expected){return $false}
  $row=@($ack.view.preferences.pages.groups.rows|Where-Object id -eq 'language')[0]
  $saved=if($Index -eq 0){$ack.view.state.settings.language -eq 'System'}else{$ack.view.state.settings.language.Explicit -eq $expected}
  $row.kind.selected -eq $Index -and $saved
 } "Language preference did not publish option $Index ($choice)" 30
 [pscustomobject]@{index=$Index;tag=$ack.view.windows_active_tag;native_name=$choice}
}
function Value($Control){$Control.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value}
function Selection($Control){@($Control.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern).GetSelection()|ForEach-Object {$_.GetText(-1)}) -join '|'}
function Select-Draft($Control,[string]$Text){
 $Control.SetFocus()
 Wait-Until {$Control.Current.HasKeyboardFocus} 'Native draft editor did not acknowledge keyboard focus' 10
 $Control.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
 Wait-Until {(Value $Control) -eq $Text} 'Native draft editor did not acknowledge the literal text' 10
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),65)
 Wait-Until {(Selection $Control) -eq $Text} 'Native text selection did not select the literal draft' 10
}
function Capture-Window($Window,[string]$Name){
 Use-Window $Window;Start-Sleep -Milliseconds 250
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $Window.hwnd -Output (Join-Path $run ($Name+'.png')) -ClientOnly -Composed *> (Join-Path $run ($Name+'-capture.json'))
 (Fresh-Model $Window)|ConvertTo-Json -Depth 80|Set-Content (Join-Path $run ($Name+'-model.json'))
}
function Application-Menu([string]$Menu){
 $caption=@((Fresh-Model).application_menus|Where-Object id -eq $Menu)[0].label
 if(!$caption){throw "Current shared application menu is missing: $Menu"}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu -Caption $caption
}
function Menu-Command([string]$Menu,[string]$Id){
 Application-Menu $Menu
 Invoke-Id $Id
}
function Key([uint16]$Code){[CapyRowPointer]::Key([uint32]$review.Id,$Code)}
function Idle{Wait-Until {$view=Model;$view -and !$view.state.document_file.busy} 'Document operation did not finish' 90}
function Choose-Path([string]$Path){
 $hit=@{edit=$null;picker=$null}
 Wait-Until {
  $hit.picker=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'#32770'))
  if(!$hit.picker){return $false}
  $hit.edit=$hit.picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit'),[System.Windows.Automation.OrCondition]::new([System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1148'),[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'1001'))))
  $null -ne $hit.edit
 } 'Native file picker did not open' 45
 if($hit.picker.Current.ProcessId -ne $review.Id -or $hit.edit.Current.ProcessId -ne $review.Id -or $hit.edit.Current.NativeWindowHandle -eq 0){throw 'Picker filename did not belong to the owned process'}
 $filenameOwner=[uint32]0;[CapyWindowApi]::GetWindowThreadProcessId([IntPtr]$hit.edit.Current.NativeWindowHandle,[ref]$filenameOwner)|Out-Null
 if($filenameOwner -ne $review.Id){throw 'Picker filename has an unexpected native owner'}
 [CapyWindowApi]::Path([IntPtr]$hit.edit.Current.NativeWindowHandle,$Path)
 Invoke-PickerButton $hit.picker
 Idle
 Wait-Until {!(Find '1148') -and !(Find '1001')} 'Native picker did not close' 45
}
function Draw([int]$Stroke,[string]$Tag){
 Menu-Command 'view' 'fit_canvas'
 Focus-Canvas
 $before=(Fresh-Model $script:current $Tag).state.document_file.revision
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $camera=Camera
 $x=[int]($bounds.X+$camera.translation[0]+$camera.zoom*(12+8*($Stroke%5)))
 $y=[int]($bounds.Y+$camera.translation[1]+$camera.zoom*(12+16*[Math]::Floor($Stroke/5)))
 [CapyRowPointer]::Down('mouse',$x,$y)
 foreach($step in 1..8){[CapyRowPointer]::Move([int]($x+$camera.zoom*$step),[int]($y+$camera.zoom*.25*$step))}
 [CapyRowPointer]::Up()
 Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $Tag -and $null -ne $view.state.document_file.revision -and $view.state.document_file.modified -and $view.state.document_file.revision -gt $before -and ($view.state.commands|Where-Object id -eq 'undo').enabled} 'Drawing after language publication did not finish its committed frame' 45
}
function Canvas-Pixels([string]$Name){
 $bounds=(Control 'drawing-canvas' -Arranged).Current.BoundingRectangle
 $window=$root.Current.BoundingRectangle
 [CapyRowPointer]::Hover([int]($window.X+20),[int]($window.Y+20))
 Start-Sleep -Milliseconds 250
 $dpi=[CapyWindowApi]::SetThreadDpiAwarenessContext([IntPtr](-4))
 $camera=Camera
 if($camera.rotation -ne 0 -or $camera.flipped[0] -or $camera.flipped[1]){throw 'Pixel comparison requires the unrotated controlled canvas'}
 $left=[int]($bounds.X+$camera.translation[0]+2*$camera.zoom);$top=[int]($bounds.Y+$camera.translation[1]+2*$camera.zoom)
 $bitmap=[Drawing.Bitmap]::new([int](60*$camera.zoom),[int](60*$camera.zoom))
 $graphics=[Drawing.Graphics]::FromImage($bitmap);$encoded=[IO.MemoryStream]::new()
 try{
  $graphics.CopyFromScreen($left,$top,0,0,$bitmap.Size)
  if($Name){$bitmap.Save((Join-Path $run ($Name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}
  $bitmap.Save($encoded,[Drawing.Imaging.ImageFormat]::Png)
  [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($encoded.ToArray()))
 }finally{$encoded.Dispose();$graphics.Dispose();$bitmap.Dispose();[CapyWindowApi]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
}
function Open-Search{
 Focus-Canvas
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),75)
 Wait-Until {(Search) -and (Find 'command-search' -Visible) -and [System.Windows.Automation.AutomationElement]::FocusedElement.Current.AutomationId -eq 'command-search'} 'Command search did not open with native text focus' 15
}
function Export-Image([string]$Path){
 Menu-Command 'file' 'export_document'
 Wait-Until {(Model).windows_document.stage -eq 'options'} 'Export options did not open' 45
 Invoke 'PrimaryButton'
 Wait-Until {(Model).windows_document.stage -eq 'preview'} 'Export preview did not finish' 90
 Invoke 'PrimaryButton';Choose-Path $Path
 Wait-Until {Test-Path -LiteralPath $Path} 'Image export did not write the Unicode path' 45
 $bytes=[IO.File]::ReadAllBytes($Path)
 if($bytes.Length -lt 45 -or [Convert]::ToHexString($bytes[0..7]) -ne '89504E470D0A1A0A' -or [Convert]::ToHexString($bytes[12..23]) -ne '494844520000004000000040'){throw 'Export was not a complete 64 by 64 PNG'}
 (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash
}
try{
 Enter-CapyEnvironment
 $previousTextScale=Text-Scale
 if($LargeText){
  $textSizeSaved=Get-ItemProperty -LiteralPath $textSizePath -Name TextScaleFactor -ErrorAction SilentlyContinue
  New-Item -Path $textSizePath -Force|Out-Null
  Set-ItemProperty -LiteralPath $textSizePath -Name TextScaleFactor -Type DWord -Value 150
  $textSizeChanged=$true;[CapyLocaleLayout]::NotifyTextSize()
  Wait-Until {[Math]::Abs((Text-Scale)-1.5) -lt .01} 'Windows accessibility text size did not become 150 percent' 15
 }
 $actualTextScale=Text-Scale
 $env:CAPY_STORAGE_DIR=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
 [IO.File]::WriteAllText((Settings-File),(@{language='System';theme=$Theme}|ConvertTo-Json -Depth 4))
 $review=Start-Process -FilePath $Executable -WorkingDirectory $run -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;Write-Output "Owned localization review $($review.Id): $run"
 Wait-Until {@(Windows).Count -eq 1} 'First window did not register' 45
 $first=@(Windows)[0];Ready $first;Use-Window $first
 $systemTag=(Fresh-Model).windows_active_tag
 $shippedTags=@((Read-Snapshot (Join-Path $run "bootstrap-$($review.Id)-$($first.id).json")).shipped_tags)
 Menu-Command 'file' 'new_window'
 Wait-Until {@(Windows).Count -eq 2} 'Second window did not register' 45
 $second=@(Windows|Where-Object id -ne $first.id)[0];Ready $second
 Use-Window $first;Focus-Canvas
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17,16),78)
 Wait-Until {@(Windows).Count -eq 3} 'Text-draft window did not register' 45
 $textWindow=@(Windows|Where-Object {$_.id -ne $first.id -and $_.id -ne $second.id})[0];Ready $textWindow;Use-Window $textWindow
 Menu-Command 'file' 'new_document'
 (Control 'document-width').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 (Control 'document-height').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('64')
 Invoke 'PrimaryButton';Idle
 Wait-Until {$view=Model;$view -and $view.state.tabs[-1].width -eq 64 -and $view.state.tabs[-1].height -eq 64} 'Small test drawing did not prepare' 45
 $layer=(Fresh-Model).state.layer_tools.editing_layer.id
 Invoke "layer-$layer-name";(Control "layer-$layer-name").SetFocus();Key 113
 Wait-Until {(Model).state.layer_tools.rename_layer -eq $layer} 'Unicode rename draft did not open' 15
 $layerName=Control "layer-$layer-rename"
 Select-Draft $layerName $literalName
 Focus-Canvas
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.editing_layer.label -eq $literalName -and $null -eq $view.state.layer_tools.rename_layer} 'Literal Unicode layer name did not commit at its existing focus boundary' 15
 Invoke-Id 'layer-add-mask'
 Wait-Until {$view=Model;$view -and @($view.state.layers|Where-Object {$_.id -eq $layer -and $_.has_mask}).Count -eq 1 -and (Find ('layer-'+$layer+'-mask')) -and (Find ('layer-'+$layer+'-mask-thumbnail'))} 'Private named layer did not expose its real mask controls' 30
 Invoke-Id ('layer-'+$layer+'-content')
 Wait-Until {$view=Model;$view -and $view.state.layer_tools.editing_layer.id -eq $layer -and !(@($view.state.layers|Where-Object id -eq $layer)[0].mask_selected)} 'Native content thumbnail did not select the drawing target after mask setup' 15
 Open-Search
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('rectangle select')
 Wait-Until {(Search).query -eq 'rectangle select' -and (Find 'command-result-0').Current.Name -eq ((Model).state.commands|Where-Object id -eq 'rectangle_select').label} 'Rectangle Select did not resolve in native command search' 15
 Key 13
 Wait-Until {(Model).state.layer_tools.tool.selection.kind -eq 'rectangle' -and !(Find 'command-search' -Visible)} 'Rectangle Select did not activate before the retained selection proof' 15
 Menu-Command 'select' 'select_all'
 Wait-Until {$view=Model;$view.state.layer_tools.has_selection -and $view.state.canvas_bar.context.kind -eq 'selection' -and ($view.state.canvas_bar.anchor -join ',') -eq '-1,-1,65,65'} 'Select All did not publish a nonempty canvas selection and conservative boundary-sampling bounds' 15
 $canvasSelection=(Fresh-Model).state.canvas_bar
 $workspaceCopies=@{
  'canvas-action-bar'=@('native-color-canvas-actions','common');'canvas-bar-more'=@('common-more','common')
  'layer-new'=@('command-add-layer','commands');'layer-new-group'=@('resources-layer-menu-new-group','resources')
  'layer-new-selection'=@('command-new-selection-layer','commands');'layer-add-mask'=@('resources-layer-menu-add-mask','resources')
  'layer-import'=@('command-import-image','commands');'layer-delete'=@('resources-layer-menu-delete-selected-layers','resources')
  'layer-actions'=@('workspace-control-layer-actions','workspace')
  'layer-controls'=@('native-layers-controls','common');'layer-options'=@('native-layers-blend-opacity','common')
  'layer-flags'=@('native-layers-flags','common');'layer-footer'=@('workspace-control-layer-actions','workspace')
  'layer-blend'=@('native-layers-blend','common');'layer-list'=@('workspace-panel-layers','workspace')
  'layer-alpha_lock'=@('resources-layer-menu-alpha-lock','resources');'layer-lock'=@('resources-layer-menu-lock-editing','resources')
 }
 $workspaceIdentities=Native-Identities (@($workspaceCopies.Keys)+@('layer-opacity','layer-attachment',('layer-row-'+$layer),('layer-'+$layer+'-name'),('layer-'+$layer+'-selection'),('layer-'+$layer+'-drag'),('layer-'+$layer+'-thumbnail'),('layer-'+$layer+'-mask'),('layer-'+$layer+'-mask-thumbnail')))
 Workspace-Grips
 $groups=@(((Fresh-Model).application_menus|Where-Object id -eq 'window').model.sections|ForEach-Object {$_})
 $toolbarMenu=@($groups|Where-Object {@($_.sections|ForEach-Object {$_}|Where-Object {$_.action.command -eq 'new_toolbar'}).Count})[0]
 Application-Menu 'window'
 (Control $toolbarMenu.label -Name).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 Invoke-Id 'new_toolbar'
 $rename=Control 'workspace-manager-name';$renameIdentity=$rename.GetRuntimeId() -join ':'
 Select-Draft $rename $literalName
 $renameSelection=Selection $rename;$textDocument=(Fresh-Model).state.document_file;$textGpu=(Fresh-Model).windows_gpu_generation
 Use-Window $first;Focus-Canvas
 [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),78)
 $width=Control 'document-width';$identity=$width.GetRuntimeId() -join ':'
 Select-Draft $width $numericDraft
 $numericSelection=Selection $width;$document=(Fresh-Model).state.document_file;$gpu=(Fresh-Model).windows_gpu_generation
 Use-Window $second
 if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{Menu-Command 'edit' 'settings'}
 $language=Control 'preference-choice-language';$choiceIdentity=$language.GetRuntimeId() -join ':'
 $navigationIds=@{'preference-search-toggle'=((Control 'preference-search-toggle').GetRuntimeId() -join ':')}
 foreach($page in (Fresh-Model).preferences.pages){$id='preference-page-'+$page.id;$navigationIds[$id]=(Control $id).GetRuntimeId() -join ':'}
 $languagePage=(Fresh-Model).preferences.page;$shortcutIdentities=@{}
 $inventory=@((Language-Row).kind.options)
 if($inventory.Count -ne $shippedTags.Count+1 -or !$shippedTags.Count){throw 'Language preference differs from the shared shipped-tag inventory'}
 $script:localizationSnapshotsReady=$true;$script:modelReadFailureCount=0
 $seen=@();$tags=@{}
 $explicitIndices=@(1..($inventory.Count-1))
 if($LanguageLimit -gt 0){$explicitIndices=@($explicitIndices|Select-Object -First $LanguageLimit)}
 foreach($index in $explicitIndices){
  $choice=Language-Choice $index;$tag=$choice.tag
  if($tags.ContainsKey($tag)){throw "Two Language options resolved to the same explicit tag: $tag"};$tags[$tag]=$index
  foreach($window in @($first,$textWindow)){Wait-Until {(Model $window).windows_active_tag -eq $tag} "Inactive window did not adopt $tag" 30}
  if(($language.GetRuntimeId() -join ':') -ne $choiceIdentity){throw 'Language choice replaced its native control'}
  if(($width.GetRuntimeId() -join ':') -ne $identity -or (Value $width) -ne $numericDraft -or (Selection $width) -ne $numericSelection){throw 'Language publication changed the native numeric editor, draft or selection'}
  if(($rename.GetRuntimeId() -join ':') -ne $renameIdentity -or (Value $rename) -ne $literalName -or (Selection $rename) -ne $renameSelection){throw 'Language publication changed the Unicode rename editor, draft or selection'}
  $numericView=Fresh-Model $first $tag;$textView=Fresh-Model $textWindow $tag
  if($numericView.state.document_file.epoch -ne $document.epoch -or $numericView.state.document_file.revision -ne $document.revision -or $numericView.windows_gpu_generation -ne $gpu -or $textView.state.document_file.epoch -ne $textDocument.epoch -or $textView.state.document_file.revision -ne $textDocument.revision -or $textView.windows_gpu_generation -ne $textGpu){throw ('Language publication changed a retained document or GPU owner: '+(@{numeric_before=$document;numeric_after=$numericView.state.document_file;numeric_gpu_before=$gpu;numeric_gpu_after=$numericView.windows_gpu_generation;text_before=$textDocument;text_after=$textView.state.document_file;text_gpu_before=$textGpu;text_gpu_after=$textView.windows_gpu_generation}|ConvertTo-Json -Depth 8 -Compress))}
  if(!$textView.state.layer_tools.has_selection -or $textView.state.canvas_bar.context.kind -ne $canvasSelection.context.kind -or $textView.state.canvas_bar.context.generation -ne $canvasSelection.context.generation -or ($textView.state.canvas_bar.anchor -join ',') -ne ($canvasSelection.anchor -join ',')){throw 'Language publication changed the nonempty canvas selection, context or bounds'}
  if($language.Current.Name -ne (Language-Row).title){throw 'Language accessibility name differs from the shared current copy'}
  if((Control 'preferences-heading').Current.Name -ne (Catalog-Text $tag 'settings-title' 'settings')){throw 'Visible Preferences heading stayed in an earlier language'}
  foreach($id in $navigationIds.Keys){if(((Control $id).GetRuntimeId() -join ':') -ne $navigationIds[$id]){throw 'Locale publication replaced a native Preferences navigation control'}}
  $preferenceView=Fresh-Model $script:current $tag
  foreach($page in $preferenceView.preferences.pages){if((Control ('preference-page-'+$page.id)).Current.Name -ne $page.title){throw 'Native Preferences page accessibility name differs from the shared current copy'}}
  $seen+=$choice
  Use-Window $textWindow;Check-Identities $workspaceIdentities;Check-WorkspaceGrips $textView $tag
  $attachment=Control 'layer-attachment';$attachmentCopy=$textView.state.layer_tools.attachment
  if($attachment.Current.Name -ne $attachmentCopy.label -or $attachment.Current.HelpText -ne $attachmentCopy.description){throw 'Retained attachment control did not follow shared contextual copy'}
  foreach($id in $workspaceCopies.Keys){$copy=$workspaceCopies[$id];if((Control $id).Current.Name -ne (Catalog-Text $tag $copy[0] $copy[1])){throw "Retained workspace control did not follow current canonical copy: $id"}}
  if((Control ('layer-row-'+$layer)).Current.Name -ne (Catalog-Text $tag 'native-layer-row' 'common').Replace('{ $title }',$literalName) -or (Control ('layer-'+$layer+'-name')).Current.Name -ne $literalName){throw 'Retained layer row lost its current typed caption or literal Unicode name'}
  foreach($pair in @(@('selection','native-layers-select-row-help'),@('drag','native-layers-move-layer'),@('thumbnail','native-layers-preview'),@('mask','native-layers-edit-mask'),@('mask-thumbnail','native-layers-mask-preview'))){if((Control ('layer-'+$layer+'-'+$pair[0])).Current.Name -ne (Catalog-Text $tag $pair[1] 'common')){throw 'Retained layer row selection, grip or preview caption stayed stale'}}
  $opacityCaption=(Catalog-Text $tag 'numeric-edit-label' 'common').Replace('{ $label }',(Catalog-Text $tag 'workspace-control-layer-opacity' 'workspace'))
  if((Control 'layer-opacity').Current.Name -ne $opacityCaption){throw 'Retained layer opacity numeric caption stayed in an earlier language'}
  Use-Window $first
  Wait-Until {(Control 'canvas-view-info').Current.Name -eq (Catalog-Text $tag 'menu-zoom' 'commands')} 'Zoom accessibility name did not follow the current language' 15
  Use-Window $second;Invoke-Id 'preference-page-shortcuts'
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $tag -and $view.preferences.page -eq 'shortcuts' -and (Find 'shortcuts-search')} 'Keyboard Shortcuts did not open in the current language' 15
  foreach($pair in @(@('shortcuts-search','native-shortcuts-search-shortcuts'),@('keymap-menu','native-shortcuts-keymap-options'))){
   $control=Control $pair[0];$runtime=$control.GetRuntimeId() -join ':'
   if($shortcutIdentities.ContainsKey($pair[0]) -and $shortcutIdentities[$pair[0]] -ne $runtime){throw 'Language publication replaced a native Shortcuts control'}
   $shortcutIdentities[$pair[0]]=$runtime
   Wait-Until {(Control $pair[0]).Current.Name -eq (Catalog-Text $tag $pair[1] 'common')} 'Shortcuts accessibility name differs from the canonical current copy' 15
  }
  Capture-Window $second ($tag+'-shortcuts')
  Invoke-Id ('preference-page-'+$languagePage)
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $tag -and $view.preferences.page -eq $languagePage -and (Find 'preference-choice-language' -Visible)} 'Language page did not return after Shortcuts acceptance' 15
  Capture-Window $first ($tag+'-numeric-draft');Capture-Window $textWindow ($tag+'-text-draft');Capture-Window $second ($tag+'-preferences')
  Resize-Window $second 900 720
  Capture-Window $second ($tag+'-preferences-narrow')
  if(!(Find 'preference-choice-language' -Visible) -or !(Find 'CloseButton' -Visible)){throw 'Narrow Preferences lost accessible language selection or dismissal'}
  Dismiss-NarrowPreferences
  Resize-Window $second 1500 1000
  if(Find 'settings-button' -Visible){Invoke 'settings-button'}else{Menu-Command 'edit' 'settings'}
  Invoke-Id ('preference-page-'+$languagePage)
  Wait-Until {(Find 'preference-choice-language' -Visible)} 'Language page did not return after native pointer dismissal' 15
 }
 $systemChoice=Language-Choice 0
 Language-Choice 0|Out-Null
 foreach($window in @($first,$textWindow)){Wait-Until {(Model $window).windows_active_tag -eq $systemTag} 'System choice did not reach an inactive window' 30}
 Capture-Window $second 'system-preferences'
 $lastIndex=$inventory.Count-1
 $lastChoice=Language-Choice $lastIndex
 Request-Language 1;Request-Language $lastIndex;Request-Language 1
 foreach($window in @($first,$textWindow)){Wait-Until {(Model $window).windows_active_tag -eq $seen[0].tag} 'Rapid choices left a stale context in another window' 30}
 Use-Window $first;Invoke 'CloseButton';Idle
 Use-Window $textWindow;Invoke 'CloseButton'
 Wait-Until {$view=Model;$view -and !(Find 'workspace-manager' -Visible) -and !$view.windows_workspace.prompt -and !$view.windows_workspace.busy} 'Dirty toolbar-name draft did not cancel' 15
 Focus-Canvas
 Menu-Command 'select' 'deselect'
 Wait-Until {$view=Model;$view -and !$view.state.layer_tools.has_selection} 'Deselect did not finish before pixel history journeys' 15
 [CapyRowPointer]::Initialize([uint32]$review.Id)
 Profile-Surfaces
 Tool-Surfaces
 Filter-Surfaces
 Preference-Numeric
 Shortcut-Surfaces
 Property-Surfaces
 foreach($panel in (Fresh-Model).panels){
  $pen=@($panel.tiles|Where-Object {$_.control.command -eq 'pen'})[0]
  if($pen){Invoke-Id "tile-$($panel.id)-$($pen.id)";break}
 }
 Wait-Until {$view=Model;$view -and ($view.state.commands|Where-Object id -eq 'pen').selected -and $view.brush_ready} 'Pen did not become ready for continued drawing' 45
 Open-Search
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('brush size')
 Wait-Until {(Search).query -eq 'brush size' -and (Find 'command-result-0')} 'Brush size parameter did not resolve' 15
 Key 13;Wait-Until {$parameter=(Search).parameter;$parameter -and !(Find 'command-result-0' -Visible) -and (Find 'command-search' -Visible).Current.Name -eq $parameter.label} 'Brush size parameter entry did not open' 15
 (Control 'command-search').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('1');Key 13
 Wait-Until {!(Find 'command-search' -Visible) -and (Model).state.brush.diameter -eq 1} 'Controlled one-pixel Pen size did not commit' 15
 $journeys=@()
 foreach($choice in $seen){
  Use-Window $second;Language-Choice $choice.index|Out-Null
  Wait-Until {(Model $textWindow).windows_active_tag -eq $choice.tag} 'Drawing window did not adopt the current context' 30
  Use-Window $textWindow;Focus-Canvas
  foreach($layer in @((Fresh-Model $textWindow $choice.tag).state.layers)){
   $label=Control "layer-$($layer.id)-label"
   if($label.Current.Name -ne $layer.label){throw "Layer row $($layer.id) lost its literal name after the language changed"}
  }
  Open-Search
  $search=Control 'command-search';$search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('undo')
  $query=(Fresh-Model).state.commands|Where-Object id -eq 'undo'|Select-Object -First 1
  Wait-Until {(Search).query -eq 'undo' -and (Find 'command-result-0').Current.Name -eq $query.label} 'English-alias command search did not resolve Undo after switching' 15
  $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($query.label)
  Wait-Until {(Search).query -eq $query.label -and (Find 'command-result-0').Current.Name -eq $query.label} 'Localized command search did not resolve Undo after switching' 15
  if($choice.tag -eq 'de'){
   $size=(Fresh-Model).state.commands|Where-Object id -eq 'image_size'|Select-Object -First 1
   foreach($spelling in @('Bildgröße','BILDGRÖSSE','BILDGRÖẞE','IMAGE SIZE')){
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($spelling)
    Wait-Until {(Search).query -eq $spelling -and (Find 'command-result-0').Current.Name -eq $size.label} 'German sharp-S and English-alias matching did not resolve Image Size' 15
   }
  }
  if($choice.tag -eq 'vi'){
   foreach($spelling in @($query.label.Normalize([Text.NormalizationForm]::FormD),$query.label.ToUpperInvariant().Normalize([Text.NormalizationForm]::FormD))){
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($spelling)
    Wait-Until {(Search).query -eq $spelling -and (Find 'command-result-0').Current.Name -eq $query.label} 'Vietnamese decomposed command search did not preserve accented matching' 15
   }
  }
  if($choice.tag -eq 'tr'){
   $size=(Fresh-Model).state.commands|Where-Object id -eq 'image_size'|Select-Object -First 1
   foreach($spelling in @('IMAGE SIZE','İMAGE SİZE','ımage sıze')){
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($spelling)
    Wait-Until {(Search).query -eq $spelling -and (Find 'command-result-0').Current.Name -eq $size.label} 'Turkish English-alias dotted/dotless I matching did not resolve Image Size' 15
   }
   $import=(Fresh-Model).state.commands|Where-Object id -eq 'import_image'|Select-Object -First 1
   foreach($spelling in @($import.label.ToUpperInvariant(),$import.label.ToUpperInvariant().Replace('I','İ'))){
    $search.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($spelling)
    Wait-Until {(Search).query -eq $spelling -and (Find 'command-result-0').Current.Name -eq $import.label} 'Turkish localized dotted/dotless I matching did not resolve Import Image' 15
   }
  }
  Capture-Window $textWindow ($choice.tag+'-search');Key 27
  Wait-Until {!(Find 'command-search' -Visible)} 'Search did not close' 15
  Application-Menu 'view'
  Capture-Window $textWindow ($choice.tag+'-view-menu');Key 27
  Menu-Command 'view' 'fit_canvas'
  $pixelsBefore=Wait-StablePixels {Canvas-Pixels ($choice.tag+'-pixels-before')}
  Draw $journeys.Count $choice.tag
  $pixelsDrawn=Wait-StablePixels {Canvas-Pixels ($choice.tag+'-pixels-drawn')}
  if($pixelsDrawn -eq $pixelsBefore){throw 'Continued drawing did not change presented canvas pixels'}
  $drawn=(Fresh-Model $script:current $choice.tag).state.document_file.revision
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17),90)
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $choice.tag -and $null -ne $view.state.document_file.revision -and $view.state.document_file.revision -gt $drawn} 'Undo stopped after language switching' 45
  Wait-Until {(Canvas-Pixels) -eq $pixelsBefore} 'Undo did not restore the presented canvas pixels' 45
  Canvas-Pixels ($choice.tag+'-pixels-undone')|Out-Null
  $undone=(Fresh-Model $script:current $choice.tag).state.document_file.revision
  [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17,16),90)
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $choice.tag -and $null -ne $view.state.document_file.revision -and $view.state.document_file.revision -gt $undone} 'Redo stopped after language switching' 45
  Canvas-Pixels ($choice.tag+'-pixels-redone')|Out-Null
  Wait-Until {(Canvas-Pixels) -eq $pixelsDrawn} 'Redo did not restore the presented drawing pixels' 45
  Menu-Command 'file' 'save_document_as'
  $project=Join-Path $run ($choice.tag+'-'+$literalName+'.capy');Choose-Path $project
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $choice.tag -and (Test-Path -LiteralPath $project) -and $view.state.document_file.location.uri -eq $project -and !$view.state.document_file.modified} 'Unicode project save did not acknowledge its checkpoint' 45
  $before=Export-Image (Join-Path $run ($choice.tag+'-before.png'))
  Menu-Command 'file' 'open_document';Choose-Path $project
  Wait-Until {$view=Model;$view -and $view.windows_active_tag -eq $choice.tag -and $view.state.document_file.location.uri -eq $project -and !$view.state.document_file.modified -and $view.state.layer_tools.editing_layer.label -eq $literalName} 'Unicode project reopen changed the saved name or checkpoint' 90
  $after=Export-Image (Join-Path $run ($choice.tag+'-after.png'))
  if($before -ne $after){throw 'Save/reopen changed exported drawing pixels after language switching'}
  Capture-Window $textWindow ($choice.tag+'-drawing')
  $journeys+=[pscustomobject]@{tag=$choice.tag;search='localized and English alias passed';drawing_undo_redo='passed';unicode_save_reopen_export='passed';export_sha256=$after}
 }
 Use-Window $second;Language-Choice 0|Out-Null;Language-Choice $lastIndex|Out-Null
 Invoke 'CloseButton';Wait-Until {$view=Model;$view -and !$view.preferences} 'Preferences did not close' 10
 Focus-Canvas;[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(17,16),78)
 Wait-Until {@(Windows).Count -eq 4} 'Future window did not register' 45
 $future=@(Windows|Where-Object {$_.id -ne $first.id -and $_.id -ne $second.id -and $_.id -ne $textWindow.id})[0];Ready $future
 if((Fresh-Model $future $lastChoice.tag).windows_active_tag -ne $lastChoice.tag){throw 'Future window used a stale language'}
 Capture-Window $future 'future-window'
 Wait-Until {(Read-Snapshot (Settings-File)).language.Explicit -eq $lastChoice.tag} 'Language preference did not persist to the isolated settings file' 30
 [pscustomobject]@{theme=$Theme;language_limit=$LanguageLimit;system_text_scale=$actualTextScale;narrow_preferences='900 by 720 for every locale';language_choices=$seen;explicit_language_count=$seen.Count;system_language=$systemChoice.tag;retained_numeric_draft_and_selection='passed';retained_unicode_text_draft_and_selection='passed';retained_nonempty_canvas_selection_and_bounds='passed';retained_preference_control='passed';retained_profile_source_proof_export='all exercised explicit locales passed';retained_shortcut_editor_picker_capture='all exercised explicit locales passed';retained_properties_numeric_choice_toggle_curve_color_gradient='all exercised explicit locales passed';retained_transform_numeric_anchor='all exercised explicit locales passed';sampler='current captions and raw choices after expected Blur/reentry passed; native RTI retention across Blur unavailable';inactive_windows='passed';future_window='passed';settings_persistence='passed';rapid_choices='passed';document_and_gpu_generation='passed';journeys=$journeys;scope='WinUI SDK/WARP controls, native file pickers and injected mouse/keys; genuine TSF, physical pen and hardware D3D12 frame pacing unverified';evidence=$run}|ConvertTo-Json -Depth 6|Tee-Object -FilePath (Join-Path $run 'results.json')
 $completed=$true
}catch{
 if($review -and !$review.HasExited){try{Capture-Window $script:current 'failure'}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 [CapyRowPointer]::Dispose()
 if($completed -and $review -and !$review.HasExited){Stop-Process -Id $review.Id -Force;Wait-Process -Id $review.Id -Timeout 30 -ErrorAction SilentlyContinue}
 Exit-CapyEnvironment
 $review=$null
 if($textSizeChanged){
  if($textSizeSaved){Set-ItemProperty -LiteralPath $textSizePath -Name TextScaleFactor -Type DWord -Value $textSizeSaved.TextScaleFactor}else{Remove-ItemProperty -LiteralPath $textSizePath -Name TextScaleFactor}
  [CapyLocaleLayout]::NotifyTextSize()
  Wait-Until {[Math]::Abs((Text-Scale)-$previousTextScale) -lt .01} 'Windows accessibility text size did not restore its original value' 15
 }
}
